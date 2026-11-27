use super::*;

#[test]
fn a_withdrawn_receive_announces_the_end_without_claiming_completion() {
    // ★ A sender that aborts leaves no file, so the row must go — but "completed" would tell the panel a file exists that
    // does not. The event is the same `TransferFinished` a cancel uses; only the snapshot is missing.
    let state = crate::testing::state();
    let id = TransferId::new();
    let mut receiving = snapshot(id, "active");
    receiving.direction = "receive".to_owned();
    state.transfers.record(receiving);
    let subscriber = state.events.subscribe();

    super::super::withdraw_received(&state, id);

    assert!(state.transfers.is_empty(), "the row is gone");
    let delivery = state.events.take(subscriber).expect("announced");
    assert!(matches!(
        delivery.events(),
        [synra_ipc::event::KernelEvent::TransferFinished { .. }]
    ));
}

#[test]
fn cancelling_a_transfer_that_is_not_in_flight_is_refused() {
    // ★ A cancel naming a transfer nobody is running means the panel is out of date. Answering "done" would leave it
    // believing it stopped something, so the refusal names the identifier and the panel can re-read.
    let state = crate::testing::state();

    let refused = super::super::transfer_cancel(&state, "00000000-0000-4000-8000-000000000003")
        .expect_err("nothing is in flight");

    assert_eq!(refused.code, synra_protocol::ErrorCode::TransferFailed);
}

#[test]
fn cancelling_an_incoming_transfer_leaves_the_abort_to_the_listener() {
    // ★ A command cannot send the abort: the listener's connection thread is the only writer to that link. The row stays
    // until that thread has actually told the sender, which is why the command only leaves the identifier behind.
    let state = crate::testing::state();
    let id = TransferId::new();
    let mut receive = snapshot(id, "receiving");
    receive.direction = "receive".to_owned();
    state.transfers.record(receive);

    super::super::transfer_cancel(&state, &id.to_string()).expect("the cancel is accepted");

    assert!(
        state.inbound_cancels.lock().contains(&id.to_string()),
        "the listener was not told to cancel"
    );
    assert!(
        state
            .transfers
            .list()
            .iter()
            .any(|transfer| transfer.transfer_id == id),
        "the row was withdrawn before the sender was told"
    );
}

#[test]
fn cancelling_a_send_leaves_finalization_to_its_worker() {
    let state = crate::testing::state();
    let id = TransferId::new();
    let cancel = synra_core::supervisor::Cancellation::new();
    state
        .transfers
        .record_owned(snapshot(id, "active"), None, None, cancel.clone());
    let subscriber = state.events.subscribe();

    super::super::transfer_cancel(&state, &id.to_string()).expect("it is in flight");
    assert!(cancel.is_cancelled());
    assert!(!state.transfers.is_empty(), "the worker has not exited yet");
    super::super::finish_transfer(&state, id, synra_core::transfer::TransferOutcome::Cancelled);

    assert!(
        state.transfers.is_empty(),
        "the registry no longer holds it"
    );
    assert!(
        super::super::transfer_list(&state).is_empty(),
        "and the list does not show it"
    );
    // ★ And the panel is told, or the row stays until the page is reopened.
    let delivery = state.events.take(subscriber).expect("announced");
    match delivery.events() {
        [synra_ipc::event::KernelEvent::TransferFinished { transfer_id }] => {
            assert_eq!(*transfer_id, id)
        }
        other => panic!("expected a finished transfer, got {other:?}"),
    }
}

#[test]
fn cancelling_something_that_is_not_an_identifier_is_the_callers_fault() {
    let state = crate::testing::state();

    let refused =
        super::super::transfer_cancel(&state, "not-an-id").expect_err("it does not parse");

    assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument);
}

#[test]
fn worker_exit_removes_its_file_after_cancellation() {
    // ★ The gap this closes: a cancelled message was removed from the panel and left on disk. The registry is the only place
    // that knows the path — a sent file belongs to the user and must survive, so "delete on cancel" cannot be a guess.
    let state = crate::testing::state();
    let path = std::env::temp_dir().join(format!("synra-cancel-{}.txt", std::process::id()));
    std::fs::write(&path, b"a message").expect("a file");
    let id = TransferId::new();
    let cancel = synra_core::supervisor::Cancellation::new();
    state
        .transfers
        .record_owned(snapshot(id, "active"), Some(path.clone()), None, cancel);
    let subscriber = state.events.subscribe();

    super::super::transfer_cancel(&state, &id.to_string()).expect("it is in flight");
    assert!(path.exists(), "the worker still owns its input");
    super::super::finish_transfer(&state, id, synra_core::transfer::TransferOutcome::Cancelled);

    assert!(!path.exists(), "the file went with the transfer");
    let delivery = state.events.take(subscriber).expect("announced");
    assert!(matches!(
        delivery.events(),
        [synra_ipc::event::KernelEvent::TransferFinished { .. }]
    ));
}
