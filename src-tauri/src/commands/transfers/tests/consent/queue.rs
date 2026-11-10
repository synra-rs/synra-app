use super::*;

#[test]
fn an_admitted_offer_is_published_and_retired_through_the_real_registry() {
    let state = testing::state();
    let id = TransferId::new();
    let peer = DeviceId::new().to_string();
    let subscriber = state.events.subscribe();
    let mut snapshot = super::super::snapshot(id, "offered");
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
