use super::*;

const PEER: &str = "11111111-1111-4111-8111-111111111111";

#[test]
fn upload_admission_checks_the_grant_before_creating_a_file() {
    let (_root, state) = crate::testing::file_state();
    let token = hosted_page(&state);
    let state = Arc::new(state);
    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.begin",
        &serde_json::json!({"to": PEER, "fileName": "a.txt", "totalBytes": 1}),
    )
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
    assert_eq!(refused.params["capability"], "file:transfer:send");
    assert!(!state.paths.cache_dir().join("plugin-transfers").exists());
}

#[test]
fn malformed_destination_and_unsafe_filename_are_rejected() {
    let (_root, state, token) = transfer_page();
    for (to, file, field) in [
        (PEER, "../escape.txt", "fileName"),
        (PEER, "C:escape", "fileName"),
        ("not-a-uuid", "file.txt", "to"),
    ] {
        let refused = super::super::plugin_ui_rpc(
            &state,
            &token,
            "transfer.begin",
            &serde_json::json!({"to": to, "fileName": file, "totalBytes": 1}),
        )
        .unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert_eq!(refused.params["field"], field);
    }
}

#[test]
fn the_host_limits_encoded_chunks_and_reports_the_expected_offset() {
    let (_root, state, token) = transfer_page();
    let opened = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.begin",
        &serde_json::json!({"to": PEER, "fileName": "a.txt", "totalBytes": 2}),
    )
    .unwrap();
    let id = &opened["uploadId"];
    let chunk_bytes = opened["chunkBytes"].as_u64().unwrap();
    let over_limit = "a".repeat((chunk_bytes as usize).div_ceil(3) * 4 + 4);
    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.chunk",
        &serde_json::json!({"uploadId": id, "data": over_limit, "offset": 0}),
    )
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::PayloadTooLarge);
    assert_eq!(refused.params["maxBytes"], chunk_bytes);
    let malformed = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.chunk",
        &serde_json::json!({"uploadId": id, "data": "invalid!", "offset": 0}),
    )
    .unwrap_err();
    assert_eq!(malformed.params["field"], "data");
    let out_of_order = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.chunk",
        &serde_json::json!({"uploadId": id, "data": "YQ==", "offset": 1}),
    )
    .unwrap_err();
    assert_eq!(out_of_order.params["expectedOffset"], 0);
}

#[test]
fn retired_single_call_rpc_is_unsupported() {
    let (_root, state, token) = transfer_page();
    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.send",
        &serde_json::json!({"to": PEER, "fileName": "a.txt", "data":"YQ=="}),
    )
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Unsupported);
}
