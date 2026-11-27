//! Reservations are global. Appends, file ownership and handoff are serialized per upload.

mod budget;
mod session;
mod spool;

use std::{
    collections::HashMap,
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use parking_lot::Mutex;
use synra_core::transfer::upload::{UploadEnd, UploadError, UploadPolicy};
use synra_ipc::IpcError;
use synra_plugin_ui::ValidatedSession;
use synra_protocol::{DeviceId, PluginId, TransferId};
use uuid::Uuid;

use budget::Budget;
use session::Pending;
pub(crate) use spool::UploadSpool;
#[cfg(feature = "tauri-shell")]
pub(crate) use spool::sweep_startup;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Owner {
    plugin: PluginId,
    nonce: u128,
}

impl From<&ValidatedSession> for Owner {
    fn from(session: &ValidatedSession) -> Self {
        Self {
            plugin: session.plugin_id().clone(),
            nonce: session.nonce,
        }
    }
}

#[derive(Debug)]
pub(super) enum Failure {
    Rule(UploadError),
    Io(io::Error),
    Admission(IpcError),
}

impl From<UploadError> for Failure {
    fn from(error: UploadError) -> Self {
        Self::Rule(error)
    }
}

impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug)]
struct Slot {
    owner: Owner,
    to: DeviceId,
    detached: AtomicBool,
    pending: Mutex<Pending>,
}

impl Slot {
    fn detach(&self) {
        self.detached.store(true, Ordering::Release);
    }

    fn live(&self) -> Result<(), Failure> {
        if self.detached.load(Ordering::Acquire) {
            Err(UploadError::Closed.into())
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Default)]
struct Sessions {
    closed: bool,
    active: HashMap<Uuid, Arc<Slot>>,
}

/// Upload spool reservations and live page-owned sessions.
#[derive(Debug, Default)]
pub struct Uploads {
    policy: UploadPolicy,
    slots: Mutex<Sessions>,
    budget: Budget,
}

pub(super) struct OpenUpload<'a> {
    pub(super) owner: Owner,
    pub(super) to: DeviceId,
    pub(super) file_name: &'a str,
    pub(super) total: u64,
}

impl Uploads {
    pub(super) fn begin(
        &self,
        cache: &Path,
        request: OpenUpload<'_>,
        now_ms: u64,
        still_authorized: impl FnOnce() -> bool,
    ) -> Result<(Uuid, usize), Failure> {
        let lease = self
            .budget
            .reserve(self.policy, request.owner.clone(), request.total)?;
        let id = lease.id;
        let (spool, file) = UploadSpool::create(&spool::root(cache), request.file_name, lease)?;
        let slot = Arc::new(Slot {
            owner: request.owner,
            to: request.to,
            detached: AtomicBool::new(false),
            pending: Mutex::new(Pending::new(request.total, now_ms, file, spool)),
        });
        let mut slots = self.slots.lock();
        if slots.closed || !still_authorized() {
            return Err(UploadError::Closed.into());
        }
        slots.active.insert(id, slot);
        Ok((id, self.policy.chunk_bytes))
    }

    fn lookup(&self, owner: &Owner, id: Uuid) -> Option<Arc<Slot>> {
        self.slots
            .lock()
            .active
            .get(&id)
            .filter(|slot| slot.owner == *owner)
            .cloned()
    }

    pub(super) fn chunk_limit(&self) -> usize {
        self.policy.chunk_bytes
    }

    pub(super) fn append(
        &self,
        owner: &Owner,
        id: Uuid,
        offset: u64,
        bytes: &[u8],
        now_ms: u64,
    ) -> Result<(u64, u64), Failure> {
        use std::io::Write as _;
        let slot = self.lookup(owner, id).ok_or(UploadError::Closed)?;
        let mut pending = slot.pending.lock();
        slot.live()?;
        let result = pending.append(self.policy, offset, bytes, now_ms, |file, bytes| {
            file.write_all(bytes)
        });
        let ended = pending.ended();
        drop(pending);
        if ended {
            self.slots.lock().active.remove(&id);
        }
        result
    }

    pub(super) fn finish(
        &self,
        owner: &Owner,
        id: Uuid,
        now_ms: u64,
        launch: impl FnOnce(DeviceId, UploadSpool) -> Result<TransferId, IpcError>,
    ) -> Result<TransferId, Failure> {
        let slot = self.lookup(owner, id).ok_or(UploadError::Closed)?;
        let result = {
            let mut pending = slot.pending.lock();
            slot.live()?;
            pending.finish(self.policy, now_ms, |spool| launch(slot.to, spool))
        };
        self.slots.lock().active.remove(&id);
        result
    }

    pub(super) fn abort(&self, owner: &Owner, id: Uuid) -> bool {
        let Some(slot) = self.lookup(owner, id) else {
            return false;
        };
        let aborted = slot.pending.lock().close(UploadEnd::Aborted);
        self.slots.lock().active.remove(&id);
        aborted
    }

    pub(crate) fn revoke(&self, nonce: u128) {
        let removed = {
            let mut slots = self.slots.lock();
            let owned: Vec<_> = slots
                .active
                .iter()
                .filter_map(|(id, slot)| (slot.owner.nonce == nonce).then_some(*id))
                .collect();
            owned
                .into_iter()
                .filter_map(|id| {
                    let slot = slots.active.remove(&id)?;
                    slot.detach();
                    Some(slot)
                })
                .collect::<Vec<_>>()
        };
        drop(removed);
    }

    pub(crate) fn close(&self) {
        let removed = {
            let mut slots = self.slots.lock();
            slots.closed = true;
            for slot in slots.active.values() {
                slot.detach();
            }
            std::mem::take(&mut slots.active)
        };
        drop(removed);
        self.budget.retry_cleanup();
    }

    #[cfg(any(feature = "tauri-shell", test))]
    pub(crate) fn sweep(&self, now_ms: u64) {
        let slots: Vec<_> = self
            .slots
            .lock()
            .active
            .iter()
            .map(|(id, slot)| (*id, Arc::clone(slot)))
            .collect();
        for (id, slot) in slots {
            let Some(mut pending) = slot.pending.try_lock() else {
                continue;
            };
            if pending.expire(self.policy, now_ms) {
                self.slots.lock().active.remove(&id);
            }
        }
        self.budget.retry_cleanup();
    }
}

impl Drop for Uploads {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests;
