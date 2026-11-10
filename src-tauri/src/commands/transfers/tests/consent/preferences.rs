use super::*;

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
