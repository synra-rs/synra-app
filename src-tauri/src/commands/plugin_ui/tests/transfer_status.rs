use super::*;

#[test]
fn a_transfer_still_in_flight_reports_the_state_the_contract_names() {
    let (_root, state, token) = transfer_page();
    let transfer_id = synra_protocol::ids::TransferId::new();
    state.transfers.record(synra_ipc::dto::TransferSnapshot {
        transfer_id,
        peer_device: synra_protocol::ids::DeviceId::from_uuid(
            uuid::Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("a uuid"),
        ),
        peer_name: "Laptop".to_owned(),
        direction: "send".to_owned(),
        kind: "attachment".to_owned(),
        // The sender's own word, which is not the contract's.
        state: "active".to_owned(),
        file_name: "a.txt".to_owned(),
        received_bytes: 0,
        total_bytes: 10,
        blocks_done: 1,
        blocks_total: 2,
        abort_reason: None,
        started_at_ms: crate::testing::FIXED_NOW_MS,
    });

    let answer = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.status",
        &serde_json::json!({"transferId": transfer_id.to_string()}),
    )
    .expect("a status");

    assert_eq!(answer["transferId"], transfer_id.to_string());
    assert_eq!(answer["state"], "transferring");
    assert_eq!(answer["totalBytes"], 10);
    assert_eq!(answer["blocksDone"], 1);
}

#[test]
fn an_unknown_transfer_says_it_is_not_in_flight() {
    let (_root, state, token) = transfer_page();

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.status",
        &serde_json::json!({"transferId": synra_protocol::ids::TransferId::new().to_string()}),
    )
    .expect_err("nothing by that id");

    assert_eq!(refused.code, ErrorCode::Unsupported);
}
