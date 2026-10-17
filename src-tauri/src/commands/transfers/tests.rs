//! What the transfer registry does with what a worker reports.

use super::Transfers;
use synra_ipc::dto::TransferSnapshot;
use synra_protocol::ids::{DeviceId, TransferId};

/// A snapshot a worker might report, with every field named.
fn snapshot(transfer_id: TransferId, state: &str) -> TransferSnapshot {
    TransferSnapshot {
        transfer_id,
        peer_device: DeviceId::new(),
        peer_name: "Study PC".to_owned(),
        direction: "send".to_owned(),
        kind: "plugin-bundle".to_owned(),
        state: state.to_owned(),
        file_name: "bundle.zip".to_owned(),
        received_bytes: 0,
        total_bytes: 1_024,
        blocks_done: 0,
        blocks_total: 1,
        abort_reason: None,
        started_at_ms: 1_000,
    }
}

#[test]
fn cancelling_a_transfer_that_is_not_in_flight_is_refused() {
    // ★ A cancel naming a transfer nobody is running means the panel is out of date. Answering "done" would leave it
    // believing it stopped something, so the refusal names the identifier and the panel can re-read.
    let state = crate::testing::state();

    let refused =
        super::transfer_cancel(&state, "00000000-0000-4000-8000-000000000003").expect_err("nothing is in flight");

    assert_eq!(refused.code, synra_protocol::ErrorCode::TransferFailed);
}

#[test]
fn cancelling_a_transfer_that_is_in_flight_drops_it() {
    let state = crate::testing::state();
    let id = TransferId::new();
    state.transfers.record(snapshot(id, "active"));

    super::transfer_cancel(&state, &id.to_string()).expect("it is in flight");

    assert!(state.transfers.is_empty(), "the registry no longer holds it");
    assert!(super::transfer_list(&state).is_empty(), "and the list does not show it");
}

#[test]
fn cancelling_something_that_is_not_an_identifier_is_the_callers_fault() {
    let state = crate::testing::state();

    let refused = super::transfer_cancel(&state, "not-an-id").expect_err("it does not parse");

    assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument);
}

#[test]
fn a_recorded_transfer_is_listed_and_a_forgotten_one_is_not() {
    let transfers = Transfers::new();
    assert!(transfers.is_empty());

    let id = TransferId::new();
    transfers.record(snapshot(id, "active"));
    assert_eq!(transfers.len(), 1);
    assert_eq!(transfers.list().len(), 1);

    transfers.forget(&id);
    assert!(transfers.is_empty(), "a finished transfer is not in flight");
}

#[test]
fn a_second_report_replaces_the_first_rather_than_joining_it() {
    // ★ Replacement, not merge: a field the worker stops reporting must not keep its old value, or the progress bar freezes
    // while the transfer moves.
    let transfers = Transfers::new();
    let id = TransferId::new();

    transfers.record(snapshot(id, "active"));
    transfers.record(snapshot(id, "finishing"));

    let listed = transfers.list();
    assert_eq!(listed.len(), 1, "one transfer, not two");
    assert_eq!(listed[0].state, "finishing");
}

#[test]
fn the_list_is_ordered_by_identifier_not_by_arrival() {
    // ★ Otherwise the order would be the network's, and the same two transfers would swap places between two reads — which
    // the interface draws as two rows moving for no reason.
    let transfers = Transfers::new();
    let one = TransferId::new();
    let two = TransferId::new();
    transfers.record(snapshot(one, "active"));
    transfers.record(snapshot(two, "active"));

    let ordered: Vec<TransferId> = transfers.list().into_iter().map(|t| t.transfer_id).collect();

    let (first, second) = if one < two { (one, two) } else { (two, one) };
    assert_eq!(ordered, vec![first, second], "sorted by identifier");
}
