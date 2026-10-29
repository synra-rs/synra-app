//! Bounded incoming attachment decisions shared by the listener and host commands.
//!
//! Silence refuses a file. An explicit response must start before the question's deadline and can settle it only once.
//! Preference persistence runs under that question's lock, never the whole desk's lock. Callers acquire the database
//! before entering `respond`; neither waiting nor announcing an offer holds the database or registry lock.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

/// How long an unanswered prompt waits before refusing the file.
pub const OFFER_TIMEOUT: Duration = Duration::from_secs(120);
/// A peer cannot accumulate an unbounded queue of questions on an unattended device.
pub const MAX_PENDING_OFFERS: usize = 32;

/// What the prompt shows about one incoming transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The sender's transfer identifier.
    pub transfer_id: String,
    /// The authenticated sender.
    pub peer: String,
    /// Proposed display name.
    pub file_name: String,
    /// Proposed byte count.
    pub total_bytes: u64,
    /// Arrival time for display.
    pub started_at_ms: i64,
}

#[derive(Debug, Default)]
struct Decision {
    answer: Option<bool>,
    active: bool,
}

#[derive(Debug)]
struct Question {
    peer: String,
    deadline: Instant,
    decision: Mutex<Decision>,
    changed: Condvar,
}

impl Question {
    #[allow(clippy::significant_drop_tightening)]
    fn wait(&self) -> bool {
        let mut decision = self.decision.lock();
        loop {
            if let Some(answer) = decision.answer {
                return answer;
            }
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            self.changed.wait_for(&mut decision, remaining);
        }
    }
}

/// Incoming questions. Each question has its own decision lock and wakeup.
#[derive(Debug, Default)]
pub struct TransferDesk {
    waiting: Mutex<HashMap<String, Arc<Question>>>,
}

/// Retires exactly this registration, including if the announcement panics.
struct Registration<'a> {
    desk: &'a TransferDesk,
    id: &'a str,
    question: Arc<Question>,
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.question.decision.lock().active = false;
        let mut waiting = self.desk.waiting.lock();
        if waiting
            .get(self.id)
            .is_some_and(|entry| Arc::ptr_eq(entry, &self.question))
        {
            waiting.remove(self.id);
        }
    }
}

impl TransferDesk {
    /// An empty question registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Announces an admitted question and waits. `None` means duplicate ID or full queue: nothing was announced.
    /// The registration exists before announcing, but no lock is held while calling the announcement. An announcement
    /// guard returned by the callback drops before the registry slot, so retirement cannot remove a newer same-ID offer.
    pub fn ask<T>(
        &self,
        pending: &Pending,
        announce: impl FnOnce() -> T,
        timeout: Duration,
    ) -> Option<bool> {
        let question = Arc::new(Question {
            peer: pending.peer.clone(),
            deadline: Instant::now() + timeout,
            decision: Mutex::new(Decision {
                answer: None,
                active: true,
            }),
            changed: Condvar::new(),
        });
        {
            let mut waiting = self.waiting.lock();
            if waiting.len() >= MAX_PENDING_OFFERS || waiting.contains_key(&pending.transfer_id) {
                return None;
            }
            waiting.insert(pending.transfer_id.clone(), Arc::clone(&question));
        }
        let _registration = Registration {
            desk: self,
            id: &pending.transfer_id,
            question: Arc::clone(&question),
        };
        let _announcement = announce();
        Some(question.wait())
    }

    /// Settles a valid question exactly once, after `prepare` succeeds. Failure leaves it unanswered for explicit retry.
    ///
    /// The deadline is checked while taking ownership of the decision; a response that starts in time can finish its
    /// small database operation after it. Acquire the database before calling, and never do network/plugin work here.
    ///
    /// # Errors
    /// Returns `prepare`'s error without changing the answer. Missing, expired and already answered questions return false
    /// without invoking `prepare`, so a stale response cannot persist a preference.
    pub fn respond<E>(
        &self,
        transfer_id: &str,
        accept: bool,
        prepare: impl FnOnce(&str) -> Result<(), E>,
    ) -> Result<bool, E> {
        let question = self.waiting.lock().get(transfer_id).cloned();
        let Some(question) = question else {
            return Ok(false);
        };
        let mut decision = question.decision.lock();
        if !decision.active || decision.answer.is_some() || Instant::now() >= question.deadline {
            return Ok(false);
        }
        prepare(&question.peer)?;
        decision.answer = Some(accept);
        drop(decision);
        question.changed.notify_one();
        Ok(true)
    }

    /// Currently registered questions, including answered ones whose waiter has not retired them yet.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.waiting.lock().len()
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_PENDING_OFFERS, Pending, TransferDesk};
    use std::sync::{Arc, mpsc};
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

    fn answer(desk: &TransferDesk, id: &str, accept: bool) -> bool {
        desk.respond(id, accept, |_| Ok::<_, ()>(())).unwrap()
    }

    #[test]
    fn an_answer_reaches_the_waiter_and_keeps_the_peer_bound_to_it() {
        for accept in [false, true] {
            let desk = Arc::new(TransferDesk::new());
            let waiter = Arc::clone(&desk);
            let (ready, announced) = mpsc::channel();
            let asked = thread::spawn(move || {
                waiter.ask(
                    &pending("one"),
                    || ready.send(()).unwrap(),
                    Duration::from_secs(5),
                )
            });
            announced.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(
                desk.respond("one", accept, |peer| {
                    assert_eq!(peer, "peer");
                    Ok::<_, ()>(())
                })
                .unwrap()
            );
            assert_eq!(asked.join().unwrap(), Some(accept));
            assert_eq!(desk.waiting(), 0);
        }
    }

    #[test]
    fn silence_and_unknown_ids_never_accept_or_prepare() {
        let desk = TransferDesk::new();
        assert_eq!(
            desk.ask(
                &pending("expired"),
                || {
                    assert!(
                        !desk
                            .respond("expired", true, |_| -> Result<(), ()> {
                                panic!("expired preference")
                            })
                            .unwrap()
                    );
                },
                Duration::ZERO
            ),
            Some(false)
        );
        assert!(
            !desk
                .respond("unknown", true, |_| -> Result<(), ()> {
                    panic!("unknown preference")
                })
                .unwrap()
        );
        assert_eq!(desk.waiting(), 0);
    }

    #[test]
    fn an_answer_cannot_be_overwritten_before_the_waiter_consumes_it() {
        let desk = TransferDesk::new();
        assert_eq!(
            desk.ask(
                &pending("one"),
                || {
                    assert!(answer(&desk, "one", false));
                    assert!(
                        !desk
                            .respond("one", true, |_| -> Result<(), ()> {
                                panic!("duplicate preference")
                            })
                            .unwrap()
                    );
                },
                Duration::from_secs(5)
            ),
            Some(false)
        );
    }

    #[test]
    fn preparation_failure_keeps_the_question_available_for_explicit_retry() {
        let desk = TransferDesk::new();
        assert_eq!(
            desk.ask(
                &pending("one"),
                || {
                    assert_eq!(
                        desk.respond("one", true, |_| Err("write refused")),
                        Err("write refused")
                    );
                    assert!(answer(&desk, "one", false));
                },
                Duration::from_secs(5)
            ),
            Some(false)
        );
    }

    #[test]
    fn duplicate_registration_neither_announces_nor_replaces_the_original() {
        let desk = TransferDesk::new();
        assert_eq!(
            desk.ask(
                &pending("one"),
                || {
                    assert_eq!(
                        desk.ask(
                            &pending("one"),
                            || panic!("duplicate announcement"),
                            Duration::ZERO
                        ),
                        None
                    );
                    assert!(answer(&desk, "one", true));
                },
                Duration::from_secs(5)
            ),
            Some(true)
        );
    }

    #[test]
    fn a_panicking_announcement_releases_its_slot() {
        let desk = TransferDesk::new();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| desk.ask(
                &pending("one"),
                || panic!("announcement failed"),
                Duration::from_secs(5)
            )))
            .is_err()
        );
        assert_eq!(desk.waiting(), 0);
        assert!(!answer(&desk, "one", true));
    }

    #[test]
    fn an_announcement_retires_before_its_identifier_can_be_reused() {
        struct Notice<'a>(&'a TransferDesk);
        impl Drop for Notice<'_> {
            fn drop(&mut self) {
                assert_eq!(self.0.waiting(), 1);
                assert_eq!(
                    self.0
                        .ask(&pending("one"), || panic!("newer notice"), Duration::ZERO),
                    None
                );
            }
        }
        let desk = TransferDesk::new();
        assert_eq!(
            desk.ask(&pending("one"), || Notice(&desk), Duration::ZERO),
            Some(false)
        );
        assert_eq!(desk.waiting(), 0);
    }

    #[test]
    fn preparation_does_not_hold_the_registry_or_another_question_lock() {
        let desk = Arc::new(TransferDesk::new());
        let waiter = Arc::clone(&desk);
        let (ready, announced) = mpsc::channel();
        let asked = thread::spawn(move || {
            waiter.ask(
                &pending("one"),
                || ready.send(()).unwrap(),
                Duration::from_secs(5),
            )
        });
        announced.recv_timeout(Duration::from_secs(2)).unwrap();
        let responder = Arc::clone(&desk);
        let (entered, preparing) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let responding = thread::spawn(move || {
            responder.respond("one", true, |_| {
                entered.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok::<_, ()>(())
            })
        });
        preparing.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            desk.ask(
                &pending("two"),
                || {
                    assert!(answer(&desk, "two", false));
                },
                Duration::from_secs(5)
            ),
            Some(false)
        );
        release.send(()).unwrap();
        assert!(responding.join().unwrap().unwrap());
        assert_eq!(asked.join().unwrap(), Some(true));
    }

    #[test]
    fn a_full_queue_rejects_without_announcing_and_slots_are_reusable() {
        let desk = Arc::new(TransferDesk::new());
        let (ready, announced) = mpsc::channel();
        let mut waiters = Vec::new();
        for index in 0..MAX_PENDING_OFFERS {
            let waiter = Arc::clone(&desk);
            let ready = ready.clone();
            waiters.push(thread::spawn(move || {
                waiter.ask(
                    &pending(&index.to_string()),
                    || ready.send(()).unwrap(),
                    Duration::from_secs(10),
                )
            }));
        }
        for _ in 0..MAX_PENDING_OFFERS {
            announced.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        assert_eq!(
            desk.ask(
                &pending("overflow"),
                || panic!("full queue announcement"),
                Duration::ZERO
            ),
            None
        );
        for index in 0..MAX_PENDING_OFFERS {
            assert!(answer(&desk, &index.to_string(), false));
        }
        for waiter in waiters {
            assert_eq!(waiter.join().unwrap(), Some(false));
        }
        assert_eq!(desk.waiting(), 0);
        assert_eq!(
            desk.ask(&pending("new"), || {}, Duration::ZERO),
            Some(false)
        );
    }
}
