use crate::commands::transfers::remember_peer;

use super::*;

#[test]
fn unpairing_during_a_prompt_revokes_automatic_and_explicit_acceptance() {
    let state = testing::state();
    let peer = DeviceId::new().to_string();
    pair(&state, &peer);
    remember_peer(&state.db(), &peer).unwrap();
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
            remember_peer(&state.db(), &peer).unwrap();
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
    remember_peer(&state.db(), &peer).unwrap();
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
    remember_peer(&state.db(), &peer).unwrap();
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
