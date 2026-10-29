//! Real command, SQLite, restart and cross-command consent tests.

use std::sync::{Barrier, mpsc};
use std::thread;
use std::time::Duration;

use synra_ipc::event::KernelEvent;
use synra_protocol::{DeviceId, ErrorCode, TransferId};

use crate::commands::{config, devices, transfers};
use crate::state::AppState;
use crate::testing;
use crate::transfer_desk::Pending;

pub(super) fn pair(state: &AppState, peer: &str) {
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: peer.to_owned(),
            display_name: "Phone".to_owned(),
            platform: "test".to_owned(),
            features: "[]".to_owned(),
            paired_at_ms: 1,
            link_token: "test-token".to_owned(),
            address: None,
        })
        .unwrap();
}

fn pending(id: &str, peer: &str) -> Pending {
    Pending {
        transfer_id: id.to_owned(),
        peer: peer.to_owned(),
        file_name: "notes.txt".to_owned(),
        total_bytes: 3,
        started_at_ms: 1,
    }
}

fn with_offer(state: &AppState, peer: &str, action: impl FnOnce(&str)) -> bool {
    let id = TransferId::new().to_string();
    let question = pending(&id, peer);
    thread::scope(|scope| {
        let (ready, announced) = mpsc::channel();
        let asked = scope.spawn(move || {
            state.transfer_desk.ask(
                &question,
                || ready.send(()).unwrap(),
                Duration::from_secs(5),
            )
        });
        announced.recv_timeout(Duration::from_secs(2)).unwrap();
        action(&id);
        asked.join().unwrap().expect("offer admitted")
    })
}

#[test]
fn remembering_a_real_response_survives_restart_and_notifies_settings() {
    let (_root, state) = testing::file_state();
    let peer = DeviceId::new().to_string();
    pair(&state, &peer);
    let subscriber = state.events.subscribe();
    assert!(with_offer(&state, &peer, |id| {
        transfers::transfer_respond(&state, id, true, Some(true)).unwrap();
        assert!(transfers::auto_accept_peer(&state, &peer));
    }));
    let delivery = state.events.take(subscriber).unwrap();
    assert!(
        matches!(delivery.events(), [KernelEvent::ConfigUpdated(snapshot)] if snapshot.values[transfers::ACCEPT_FROM_KEY] == serde_json::json!([peer]))
    );
    let reopened = testing::reopen(&state);
    assert!(transfers::auto_accept_peer(&reopened, &peer));
}

#[test]
fn a_sqlite_write_failure_does_not_answer_or_announce_and_can_be_retried() {
    let (_root, state) = testing::file_state();
    let peer = DeviceId::new().to_string();
    pair(&state, &peer);
    let db = rusqlite::Connection::open(state.paths.db_path()).unwrap();
    db.execute_batch("CREATE TRIGGER refuse_remember BEFORE INSERT ON config WHEN NEW.key = 'transfers.acceptFrom' BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    let subscriber = state.events.subscribe();
    assert!(with_offer(&state, &peer, |id| {
        assert_eq!(
            transfers::transfer_respond(&state, id, true, Some(true))
                .unwrap_err()
                .code,
            ErrorCode::Internal
        );
        assert_eq!(state.transfer_desk.waiting(), 1);
        assert!(!transfers::auto_accept_peer(&state, &peer));
        assert!(state.events.take(subscriber).is_none());
        db.execute_batch("DROP TRIGGER refuse_remember").unwrap();
        transfers::transfer_respond(&state, id, true, Some(true)).unwrap();
    }));
    assert!(transfers::auto_accept_peer(&state, &peer));
    assert!(matches!(
        state.events.take(subscriber).unwrap().events(),
        [KernelEvent::ConfigUpdated(_)]
    ));
}

#[test]
fn expired_and_repeated_answers_cannot_persist_permission() {
    let state = testing::state();
    let peer = DeviceId::new().to_string();
    pair(&state, &peer);
    let id = TransferId::new().to_string();
    assert_eq!(
        state.transfer_desk.ask(
            &pending(&id, &peer),
            || {
                assert_eq!(
                    transfers::transfer_respond(&state, &id, true, Some(true))
                        .unwrap_err()
                        .code,
                    ErrorCode::TransferFailed
                );
            },
            Duration::ZERO
        ),
        Some(false)
    );
    assert!(!transfers::auto_accept_peer(&state, &peer));
    let id = TransferId::new().to_string();
    assert_eq!(
        state.transfer_desk.ask(
            &pending(&id, &peer),
            || {
                transfers::transfer_respond(&state, &id, false, Some(true)).unwrap();
                assert_eq!(
                    transfers::transfer_respond(&state, &id, true, Some(true))
                        .unwrap_err()
                        .code,
                    ErrorCode::TransferFailed
                );
            },
            Duration::from_secs(5)
        ),
        Some(false)
    );
    assert!(!transfers::auto_accept_peer(&state, &peer));
}

#[test]
fn concurrent_responses_merge_preferences_instead_of_overwriting_them() {
    let state = testing::state();
    let peers = [DeviceId::new().to_string(), DeviceId::new().to_string()];
    for peer in &peers {
        pair(&state, peer);
    }
    let start = Barrier::new(2);
    thread::scope(|scope| {
        let workers: Vec<_> = peers
            .iter()
            .map(|peer| {
                scope.spawn(|| {
                    assert!(with_offer(&state, peer, |id| {
                        start.wait();
                        transfers::transfer_respond(&state, id, true, Some(true)).unwrap();
                    }));
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
    });
    let stored = config::config_get(&state).unwrap();
    assert_eq!(
        stored.values[transfers::ACCEPT_FROM_KEY]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for peer in &peers {
        assert!(transfers::auto_accept_peer(&state, peer));
    }
}

#[test]
fn malformed_preferences_are_not_silently_replaced_when_remembering() {
    let state = testing::state();
    let peer = DeviceId::new().to_string();
    pair(&state, &peer);
    state
        .db()
        .set_config(transfers::ACCEPT_FROM_KEY, &serde_json::json!("corrupt"), 1)
        .unwrap();
    assert!(!transfers::auto_accept_peer(&state, &peer));
    assert!(!with_offer(&state, &peer, |id| {
        assert_eq!(
            transfers::transfer_respond(&state, id, true, Some(true))
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
        transfers::transfer_respond(&state, id, false, None).unwrap();
    }));
    assert_eq!(
        config::config_get(&state).unwrap().values[transfers::ACCEPT_FROM_KEY],
        "corrupt"
    );
}

#[test]
fn unpairing_during_a_prompt_revokes_automatic_and_explicit_acceptance() {
    let state = testing::state();
    let peer = DeviceId::new().to_string();
    pair(&state, &peer);
    super::super::remember_peer(&state.db(), &peer).unwrap();
    assert!(!with_offer(&state, &peer, |id| {
        devices::devices_forget(&state, &peer).unwrap();
        assert_eq!(
            transfers::transfer_respond(&state, id, true, Some(true))
                .unwrap_err()
                .code,
            ErrorCode::TransferFailed
        );
        assert!(!transfers::auto_accept_peer(&state, &peer));
        transfers::transfer_respond(&state, id, false, None).unwrap();
    }));
    assert!(
        config::config_get(&state)
            .unwrap()
            .values
            .get(transfers::ACCEPT_FROM_KEY)
            .is_none()
    );
}

#[test]
fn an_unpaired_or_blocked_sender_cannot_be_accepted() {
    let (_root, state) = testing::file_state();
    let peer = DeviceId::new().to_string();
    let db = rusqlite::Connection::open(state.paths.db_path()).unwrap();
    for blocked in [false, true] {
        if blocked {
            pair(&state, &peer);
            db.execute(
                "UPDATE paired_devices SET blocked = 1 WHERE device_id = ?1",
                [&peer],
            )
            .unwrap();
            super::super::remember_peer(&state.db(), &peer).unwrap();
        }
        assert!(!transfers::auto_accept_peer(&state, &peer));
        assert!(!with_offer(&state, &peer, |id| {
            assert_eq!(
                transfers::transfer_respond(&state, id, true, None)
                    .unwrap_err()
                    .code,
                ErrorCode::TransferFailed
            );
            transfers::transfer_respond(&state, id, false, None).unwrap();
        }));
    }
}

#[test]
fn forgetting_cannot_silently_ignore_a_preference_delete_failure() {
    let (_root, state) = testing::file_state();
    let peer = DeviceId::new().to_string();
    pair(&state, &peer);
    super::super::remember_peer(&state.db(), &peer).unwrap();
    let db = rusqlite::Connection::open(state.paths.db_path()).unwrap();
    db.execute_batch("CREATE TRIGGER refuse_forget BEFORE DELETE ON config BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    let subscriber = state.events.subscribe();
    assert_eq!(
        devices::devices_forget(&state, &peer).unwrap_err().code,
        ErrorCode::Internal
    );
    assert!(state.db().is_paired_with(&peer).unwrap());
    assert!(transfers::auto_accept_peer(&state, &peer));
    assert!(state.events.take(subscriber).is_none());
}

#[test]
fn failed_unpairing_announces_the_safer_preference_change_without_removing_the_device() {
    let (_root, state) = testing::file_state();
    let peer = DeviceId::new().to_string();
    pair(&state, &peer);
    super::super::remember_peer(&state.db(), &peer).unwrap();
    let db = rusqlite::Connection::open(state.paths.db_path()).unwrap();
    db.execute_batch("CREATE TRIGGER refuse_unpair BEFORE DELETE ON paired_devices BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    let subscriber = state.events.subscribe();
    assert_eq!(
        devices::devices_forget(&state, &peer).unwrap_err().code,
        ErrorCode::Internal
    );
    assert!(state.db().is_paired_with(&peer).unwrap());
    assert!(!transfers::auto_accept_peer(&state, &peer));
    assert!(
        matches!(state.events.take(subscriber).unwrap().events(), [KernelEvent::ConfigUpdated(snapshot)] if snapshot.values.get(transfers::ACCEPT_FROM_KEY).is_none())
    );
}

#[test]
fn settings_refuse_invalid_or_unbounded_automatic_acceptance_lists() {
    let state = testing::state();
    let peer = DeviceId::new().to_string();
    for value in [
        serde_json::json!(true),
        serde_json::json!(["not-a-device"]),
        serde_json::json!(vec![peer.clone(); 257]),
    ] {
        let patch = synra_ipc::ConfigPatch {
            set: [(transfers::ACCEPT_FROM_KEY.to_owned(), value)]
                .into_iter()
                .collect(),
            unset: Vec::new(),
        };
        assert_eq!(
            config::config_set(&state, &patch).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
    assert!(
        config::config_get(&state)
            .unwrap()
            .values
            .get(transfers::ACCEPT_FROM_KEY)
            .is_none()
    );
}

#[test]
fn an_admitted_offer_is_published_and_retired_through_the_real_registry() {
    let state = testing::state();
    let id = TransferId::new();
    let peer = DeviceId::new().to_string();
    let subscriber = state.events.subscribe();
    let mut snapshot = super::snapshot(id, "offered");
    snapshot.direction = "receive".to_owned();
    assert_eq!(
        state.transfer_desk.ask(
            &pending(&id.to_string(), &peer),
            || {
                let notice = transfers::announce_offer(&state, snapshot);
                assert_eq!(transfers::transfer_list(&state).len(), 1);
                notice
            },
            Duration::ZERO
        ),
        Some(false)
    );
    assert!(transfers::transfer_list(&state).is_empty());
    assert!(
        matches!(state.events.take(subscriber).unwrap().events(), [KernelEvent::TransferUpdated(_), KernelEvent::TransferFinished { transfer_id }] if *transfer_id == id)
    );
}

#[test]
fn a_response_delayed_behind_database_access_cannot_remember_an_expired_offer() {
    let state = testing::state();
    let peer = DeviceId::new().to_string();
    pair(&state, &peer);
    let id = TransferId::new().to_string();
    thread::scope(|scope| {
        let held = state.db();
        let (start, starting) = mpsc::channel();
        let (entered, entering) = mpsc::channel();
        let state_ref = &state;
        let id_ref = &id;
        let responding = scope.spawn(move || {
            starting.recv_timeout(Duration::from_secs(2)).unwrap();
            entered.send(()).unwrap();
            transfers::transfer_respond(state_ref, id_ref, true, Some(true))
        });
        assert_eq!(
            state.transfer_desk.ask(
                &pending(&id, &peer),
                || {
                    start.send(()).unwrap();
                    entering.recv_timeout(Duration::from_secs(2)).unwrap();
                },
                Duration::ZERO
            ),
            Some(false)
        );
        drop(held);
        assert_eq!(
            responding.join().unwrap().unwrap_err().code,
            ErrorCode::TransferFailed
        );
    });
    assert!(!transfers::auto_accept_peer(&state, &peer));
}

#[test]
fn concurrent_unpairing_and_acceptance_cannot_leave_a_trusted_unpaired_sender() {
    let state = testing::state();
    let peer = DeviceId::new().to_string();
    pair(&state, &peer);
    with_offer(&state, &peer, |id| {
        let start = Barrier::new(2);
        thread::scope(|scope| {
            let forgetting = scope.spawn(|| {
                start.wait();
                devices::devices_forget(&state, &peer).unwrap();
            });
            start.wait();
            let response = transfers::transfer_respond(&state, id, true, Some(true));
            forgetting.join().unwrap();
            if response.is_err() {
                transfers::transfer_respond(&state, id, false, None).unwrap();
            }
        });
    });
    assert!(!state.db().is_paired_with(&peer).unwrap());
    assert!(
        config::config_get(&state)
            .unwrap()
            .values
            .get(transfers::ACCEPT_FROM_KEY)
            .is_none()
    );
}
