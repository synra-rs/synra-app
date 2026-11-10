use super::*;

#[test]
fn an_incoming_step_becomes_a_receive_row() {
    // ★ `direction` is the field that makes the panel say "from" rather than "to"; a receiving row without it would be
    // drawn as a send, which is the one wrong answer a user would act on.
    let id = TransferId::new();

    let row = super::super::received_snapshot(&progress_step(id, None, 512, 1), "Study PC", 1_000);

    assert_eq!(row.transfer_id, id);
    assert_eq!(row.direction, "receive");
    assert_eq!(row.kind, "attachment");
    assert_eq!(row.state, "active");
    assert_eq!(row.peer_name, "Study PC");
    assert_eq!(row.file_name, "photo.png");
    assert_eq!(row.received_bytes, 512);
    assert_eq!(row.total_bytes, 2_048);
    assert_eq!(row.blocks_done, 1);
    assert_eq!(row.blocks_total, 4);
}

#[test]
fn a_bundle_in_the_making_is_named_as_one() {
    let row = super::super::received_snapshot(
        &progress_step(TransferId::new(), Some("@acme/chat"), 0, 0),
        "Laptop",
        1_000,
    );

    assert_eq!(row.kind, "plugin-bundle");
}

#[test]
fn finishing_an_incoming_transfer_closes_its_row_and_announces_it() {
    // ★ The receiving half of `finish_transfer`: nothing else will remove the row, because a received transfer has no
    // worker that owns it. The completed snapshot comes first so the panel can draw 100% before the row goes.
    let state = crate::testing::state();
    let id = TransferId::new();
    state.transfers.record(snapshot(id, "active"));
    let subscriber = state.events.subscribe();

    super::super::complete_received(&state, id);

    assert!(state.transfers.is_empty(), "the row is gone");
    let delivery = state.events.take(subscriber).expect("announced");
    let events = delivery.events();
    assert!(
        matches!(events.first(), Some(synra_ipc::event::KernelEvent::TransferUpdated(row)) if row.state == "completed"),
        "the completed row is not announced first: {events:?}"
    );
    assert!(
        matches!(events.last(), Some(synra_ipc::event::KernelEvent::TransferFinished { transfer_id }) if *transfer_id == id),
        "the panel is not told the transfer ended: {events:?}"
    );
}

#[test]
fn an_inbound_attempt_is_counted_and_only_a_received_one_has_a_size() {
    // ★ The receiving half of the transfer metrics, tested where the decision lives rather than through a socket: an
    // aborted transfer is still a transfer (the user asked "did it arrive"), but the size histogram describes files that
    // arrived, so it contributes none.
    use synra_observe::metrics::Value;

    let state = crate::testing::state();
    super::super::record_received(
        &state,
        "attachment",
        512,
        Some(std::time::Duration::from_millis(4)),
        true,
    );
    super::super::record_received(
        &state,
        "plugin-bundle",
        1_024,
        Some(std::time::Duration::from_millis(8)),
        false,
    );

    let snapshot = state.metrics.snapshot();
    assert_eq!(
        snapshot.get("transfers_total{kind=attachment,outcome=aborted}"),
        Some(&Value::Counter(1)),
        "an aborted transfer is still counted"
    );
    assert_eq!(
        snapshot.get("transfers_total{kind=plugin-bundle,outcome=received}"),
        Some(&Value::Counter(1)),
        "a received bundle is counted as one"
    );
    assert_eq!(
        snapshot.get("transfer_bytes{kind=attachment}"),
        None,
        "an aborted transfer has no size"
    );
    match snapshot.get("transfer_bytes{kind=plugin-bundle}") {
        Some(Value::Histogram(histogram)) => {
            assert_eq!(histogram.count, 1);
            assert_eq!(histogram.sum, 1_024.0);
        }
        other => panic!("expected a size histogram, got {other:?}"),
    }
    assert!(
        snapshot
            .get("transfer_duration_ms{kind=attachment}")
            .is_some(),
        "an abort still took time from its first progress event"
    );
    assert!(
        snapshot
            .get("transfer_throughput_bps{kind=attachment}")
            .is_none(),
        "an aborted transfer has no rate: it did not move its bytes"
    );
}
