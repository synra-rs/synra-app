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
