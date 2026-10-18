//! The incoming transfers waiting for this device's answer.
//!
//! # Why a desk and not a field on the connection
//!
//! Because the question is asked on one thread and answered on another. A transfer arrives on the listener's connection
//! thread, the prompt is drawn by the interface, and the answer comes back through a command — three places that share
//! nothing except a value they can both reach. This is the same shape as the pairing desk, and for the same reason.
//!
//! # Why a timeout accepts
//!
//! Because that is what every caller did before the prompt existed, and a prompt nobody sees must not turn into a file
//! that nobody receives. The window is long enough for a person to read it and short enough that a peer is not left
//! waiting on a screen that went to sleep. A future policy — refuse on timeout, or remember a preference — belongs here,
//! where the answer is decided, rather than at each call site.
//!
//! # What this does not do
//!
//! It does not send the peer anything. The kernel does that with [`synra_core::inbound::OfferDecision::Refuse`], and a
//! desk that also wrote to a socket would be a second place the transfer's fate is decided.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

/// How long a prompt waits before the transfer is accepted anyway.
pub const OFFER_TIMEOUT: Duration = Duration::from_secs(120);

/// What the prompt shows about one incoming transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The transfer, as the peer named it.
    pub transfer_id: String,
    /// The device that dialled.
    pub peer: String,
    /// The name the file would take.
    pub file_name: String,
    /// How many bytes it says it is.
    pub total_bytes: u64,
    /// When the proposal arrived.
    pub started_at_ms: i64,
}

#[derive(Debug)]
struct WaitingEntry {
    /// `Some(answer)` once someone has answered, `None` while still waiting.
    answer: Option<bool>,
    /// The device that asked, kept so an answer can remember it.
    peer: String,
}

#[derive(Debug, Default)]
struct Waiting {
    /// One entry per transfer.
    answers: HashMap<String, WaitingEntry>,
}

/// The transfers waiting for an answer.
#[derive(Debug, Default)]
pub struct TransferDesk {
    waiting: Mutex<Waiting>,
    changed: Condvar,
}

impl TransferDesk {
    /// An empty desk.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a question, announces it, and waits for the answer.
    ///
    /// The announcement runs **after** the entry exists, so an answer that arrives before this call is over is still
    /// found — the opposite order would drop a prompt answered as it appeared. It runs before the wait, because a prompt
    /// announced after the wait would be shown after the transfer already arrived.
    // The guard is deliberately held across the wait loop: `wait_for` needs it to unlock while it sleeps, and dropping it
    // between iterations would leave the entry visible to a `respond` the loop then never observes.
    #[allow(clippy::significant_drop_tightening)]
    pub fn ask(
        &self,
        pending: &Pending,
        announce: impl FnOnce(),
        timeout: Duration,
    ) -> bool {
        let mut waiting = self.waiting.lock();
        waiting.answers.insert(
            pending.transfer_id.clone(),
            WaitingEntry { answer: None, peer: pending.peer.clone() },
        );
        announce();

        let deadline = Instant::now() + timeout;
        loop {
            if let Some(answer) = waiting
                .answers
                .get(&pending.transfer_id)
                .and_then(|entry| entry.answer)
            {
                waiting.answers.remove(&pending.transfer_id);
                return answer;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                waiting.answers.remove(&pending.transfer_id);
                // The timeout policy: see the module docs. Nothing has been decided, and the transfer is accepted.
                return true;
            }
            self.changed.wait_for(&mut waiting, remaining);
        }
    }

    /// Answers a waiting question, answering whether one was waiting.
    ///
    /// `false` is the ordinary answer to a prompt that was already settled (the interface repeated a click) or that
    /// expired while the user was deciding.
    pub fn respond(&self, transfer_id: &str, accept: bool) -> bool {
        let answered = {
            let mut waiting = self.waiting.lock();
            match waiting.answers.get_mut(transfer_id) {
                Some(entry) => {
                    entry.answer = Some(accept);
                    true
                }
                None => false,
            }
        };
        // Notified after the lock is released: the waiter re-checks under the lock anyway, and this is one fewer thread
        // that can be woken only to block on a guard the notifier still holds.
        if answered {
            self.changed.notify_all();
        }
        answered
    }

    /// The device waiting for an answer to this transfer, if one is.
    ///
    /// Used to remember a sender when the user chooses "always accept": the peer is part of the question, and a `respond`
    /// that only carried the answer would have lost it.
    #[must_use]
    pub fn peer_of(&self, transfer_id: &str) -> Option<String> {
        self.waiting
            .lock()
            .answers
            .get(transfer_id)
            .map(|entry| entry.peer.clone())
    }

    /// How many questions are waiting, for a diagnostic or a test.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.waiting.lock().answers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{Pending, TransferDesk};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    fn pending(id: &str) -> Pending {
        Pending {
            transfer_id: id.to_owned(),
            peer: "peer".to_owned(),
            file_name: "notes.txt".to_owned(),
            total_bytes: 3,
            started_at_ms: 1,
        }
    }

    #[test]
    fn an_answer_reaches_the_thread_that_is_waiting() {
        let desk = Arc::new(TransferDesk::new());
        let waiter = Arc::clone(&desk);
        let question = pending("one");
        let asked = thread::spawn(move || waiter.ask(&question, || {}, Duration::from_secs(5)));

        // The prompt is announced from inside `ask`, so polling is how the test learns the entry exists — the same thing
        // a person does, minus the reading.
        while desk.waiting() == 0 {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(desk.respond("one", false), "the question is waiting");
        assert!(!asked.join().expect("the thread"), "the answer was refused");
        assert_eq!(desk.waiting(), 0, "the answered question is gone");
    }

    #[test]
    fn the_peer_is_available_while_the_question_waits() {
        // ★ "Always accept this device" needs the device, and the answer alone does not carry one. The desk keeps it with
        // the question rather than the caller remembering a second map.
        let desk = Arc::new(TransferDesk::new());
        let waiter = Arc::clone(&desk);
        let question = pending("with-peer");
        let asked = thread::spawn(move || waiter.ask(&question, || {}, Duration::from_secs(5)));
        while desk.waiting() == 0 {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(desk.peer_of("with-peer"), Some("peer".to_owned()));
        assert!(desk.respond("with-peer", true));
        assert!(asked.join().expect("the thread"));
        assert_eq!(desk.peer_of("with-peer"), None, "the answered question is gone");
    }

    #[test]
    fn an_unknown_question_cannot_be_answered() {
        let desk = TransferDesk::new();
        assert!(!desk.respond("nothing", true));
    }

    #[test]
    fn a_prompt_nobody_answers_is_accepted_when_it_expires() {
        let desk = TransferDesk::new();
        // A timeout of zero is the far edge of the policy and the only one a test can reach without waiting. The answer
        // is `true`, which is the behaviour before prompts existed.
        assert!(desk.ask(&pending("slow"), || {}, Duration::ZERO));
        assert_eq!(desk.waiting(), 0);
    }
}
