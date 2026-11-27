use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use synra_ipc::IpcError;
use synra_ipc::event::{PluginEvent, PluginEventEnvelope, PluginEventPoll, PluginEventRecovery};
use synra_protocol::{ErrorCode, TransferId};
use tokio::sync::Notify;
use uuid::Uuid;

use super::selectors::{Selector, Topic};

const MAX_PATTERNS: usize = 64;
const MAX_QUEUED: usize = 8;

type Message = Result<PluginEventEnvelope, IpcError>;

/// A page has one bounded stream from its first subscription to its last unsubscribe.
#[derive(Default)]
pub(crate) struct PluginEvents {
    streams: Mutex<HashMap<u128, Arc<Stream>>>,
    pub(super) projection: Mutex<()>,
}

impl std::fmt::Debug for PluginEvents {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginEvents")
            .field("open_streams", &self.streams.lock().len())
            .finish()
    }
}

impl PluginEvents {
    pub(super) fn subscribe(
        &self,
        nonce: u128,
        id: u32,
        selector: Selector,
    ) -> Result<Uuid, IpcError> {
        let mut streams = self.streams.lock();
        let stream = streams
            .entry(nonce)
            .or_insert_with(|| Arc::new(Stream::new()));
        stream.subscribe(id, selector)?;
        Ok(stream.id)
    }

    pub(super) fn stream(&self, nonce: u128, id: Uuid) -> Option<Arc<Stream>> {
        self.streams
            .lock()
            .get(&nonce)
            .filter(|stream| stream.id == id)
            .cloned()
    }

    pub(super) fn unsubscribe(&self, nonce: u128, id: Uuid, subscription: u32) {
        let mut streams = self.streams.lock();
        let Some(stream) = streams.get(&nonce).filter(|stream| stream.id == id) else {
            return;
        };
        if stream.unsubscribe(subscription) {
            streams.remove(&nonce);
        }
    }

    pub(crate) fn close(&self, nonce: u128) {
        let stream = self.streams.lock().remove(&nonce);
        if let Some(stream) = stream {
            stream.close();
        }
    }

    pub(super) fn open(&self) -> Vec<(u128, Arc<Stream>)> {
        self.streams
            .lock()
            .iter()
            .map(|(nonce, stream)| (*nonce, Arc::clone(stream)))
            .collect()
    }
}

pub(super) struct Stream {
    id: Uuid,
    inner: Mutex<Queue>,
    wake: Notify,
    reader: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct Queue {
    subscriptions: HashMap<u32, Selector>,
    messages: VecDeque<Message>,
    seq: u64,
    missed: bool,
}

impl Queue {
    fn selects(&self, event: &PluginEvent) -> bool {
        self.subscriptions
            .values()
            .any(|selector| selector.matches(event))
    }
}

impl Stream {
    fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            inner: Mutex::new(Queue::default()),
            wake: Notify::new(),
            reader: tokio::sync::Mutex::new(()),
        }
    }

    pub(super) fn observes(&self, topic: Topic) -> bool {
        self.inner
            .lock()
            .subscriptions
            .values()
            .any(|selector| selector.observes(topic))
    }

    pub(super) fn selects_transfer(&self, id: TransferId) -> bool {
        self.inner
            .lock()
            .subscriptions
            .values()
            .any(|selector| selector.transfer(id))
    }

    fn subscribe(&self, id: u32, selector: Selector) -> Result<(), IpcError> {
        let mut queue = self.inner.lock();
        if queue.subscriptions.len() >= MAX_PATTERNS && !queue.subscriptions.contains_key(&id) {
            return Err(limit("too many logical subscriptions"));
        }
        queue.subscriptions.insert(id, selector);
        Ok(())
    }

    fn unsubscribe(&self, id: u32) -> bool {
        let mut queue = self.inner.lock();
        queue.subscriptions.remove(&id);
        if queue.subscriptions.is_empty() {
            queue.messages.clear();
            drop(queue);
            self.wake.notify_one();
            return true;
        }
        // Drop events queued only for a removed pattern. The next sequence exposes the gap.
        let Queue {
            subscriptions,
            messages,
            ..
        } = &mut *queue;
        messages.retain(|message| match message {
            Ok(envelope) => subscriptions
                .values()
                .any(|selector| selector.matches(&envelope.event)),
            Err(_) => true,
        });
        false
    }

    fn close(&self) {
        let mut queue = self.inner.lock();
        queue.subscriptions.clear();
        queue.messages.clear();
        drop(queue);
        self.wake.notify_one();
    }

    pub(super) fn publish(&self, topic: Topic, at: i64, event: Result<PluginEvent, IpcError>) {
        let mut queue = self.inner.lock();
        let interested = match &event {
            Ok(event) => queue.selects(event),
            Err(_) => queue
                .subscriptions
                .values()
                .any(|selector| selector.observes(topic)),
        };
        if !interested {
            return;
        }
        queue.seq = queue.seq.saturating_add(1);
        let message = event.map(|event| PluginEventEnvelope {
            seq: queue.seq,
            at,
            event,
        });
        if queue.messages.len() == MAX_QUEUED {
            queue.messages.pop_front();
        }
        queue.messages.push_back(message);
        drop(queue);
        self.wake.notify_one();
    }

    pub(super) fn invalidate(&self) {
        let mut queue = self.inner.lock();
        queue.missed = true;
        queue.messages.clear();
        drop(queue);
        self.wake.notify_one();
    }

    pub(super) fn reset(&self, mut events: Vec<PluginEvent>) -> PluginEventRecovery {
        let mut queue = self.inner.lock();
        events.retain(|event| queue.selects(event));
        queue.missed = false;
        queue.messages.clear();
        PluginEventRecovery {
            seq: queue.seq,
            events,
        }
    }

    /// Hold one reader across the wait. Wake immediately on a projected event or final unsubscribe.
    pub(super) async fn poll(
        &self,
        deadline: Duration,
    ) -> Result<Option<PluginEventPoll>, IpcError> {
        let _reader = self
            .reader
            .try_lock()
            .map_err(|_| limit("an event receive is already pending"))?;
        let next = async {
            loop {
                {
                    let mut queue = self.inner.lock();
                    if queue.subscriptions.is_empty() {
                        return Ok(None);
                    }
                    if queue.missed {
                        return Ok(Some(PluginEventPoll::Reconcile { resync: true }));
                    }
                    if let Some(message) = queue.messages.pop_front() {
                        return message.map(|envelope| Some(PluginEventPoll::Event(envelope)));
                    }
                }
                self.wake.notified().await;
            }
        };
        tokio::time::timeout(deadline, next)
            .await
            .unwrap_or(Ok(None))
    }
}

fn limit(reason: &str) -> IpcError {
    IpcError::unknown(ErrorCode::RateLimited).with_params(serde_json::json!({"reason": reason}))
}
