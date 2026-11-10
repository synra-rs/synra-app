use super::*;

#[test]
fn a_page_without_the_transfer_grant_cannot_send() {
    let state = crate::testing::state();
    let token = hosted_page(&state);

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.send",
        &serde_json::json!({"to": "11111111-1111-4111-8111-111111111111", "fileName": "a.txt", "data": ""}),
    )
    .expect_err("no grant");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
    assert_eq!(
        refused
            .params
            .get("capability")
            .and_then(|value| value.as_str()),
        Some("file:transfer:send")
    );
}

#[test]
fn a_transfer_argument_this_channel_cannot_honour_is_named() {
    use base64::Engine as _;

    let (_root, state, token) = transfer_page();
    let okay = base64::engine::general_purpose::STANDARD.encode(b"hello");

    // The grouping id was accepted by the signature and never reached the sender; forwarding it would be a promise.
    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.send",
        &serde_json::json!({
            "to": "11111111-1111-4111-8111-111111111111",
            "fileName": "a.txt",
            "data": okay,
            "contextId": "conversation-1",
        }),
    )
    .expect_err("grouping is not implemented");
    assert_eq!(refused.code, ErrorCode::Unsupported);

    // A name with a separator would write outside the directory this call made.
    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.send",
        &serde_json::json!({
            "to": "11111111-1111-4111-8111-111111111111",
            "fileName": "../escape.txt",
            "data": okay,
        }),
    )
    .expect_err("not one path component");
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(
        refused.params.get("field").and_then(|value| value.as_str()),
        Some("fileName")
    );

    // The device id is a uuid; a made-up string is the caller's mistake, not a failed dial.
    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.send",
        &serde_json::json!({"to": "not-a-uuid", "fileName": "a.txt", "data": okay}),
    )
    .expect_err("not a device id");
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(
        refused.params.get("field").and_then(|value| value.as_str()),
        Some("to")
    );

    // And bytes that are not base64 never become a temp file.
    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.send",
        &serde_json::json!({
            "to": "11111111-1111-4111-8111-111111111111",
            "fileName": "a.txt",
            "data": "not base64!!",
        }),
    )
    .expect_err("not base64");
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(
        refused.params.get("field").and_then(|value| value.as_str()),
        Some("data")
    );
}

#[test]
fn a_file_larger_than_one_ipc_message_is_refused_by_size() {
    use base64::Engine as _;

    let (_root, state, token) = transfer_page();
    let too_big = vec![0u8; super::super::MAX_PLUGIN_TRANSFER_BYTES + 1];
    let data = base64::engine::general_purpose::STANDARD.encode(too_big);

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.send",
        &serde_json::json!({
            "to": "11111111-1111-4111-8111-111111111111",
            "fileName": "big.bin",
            "data": data,
        }),
    )
    .expect_err("over the cap");

    assert_eq!(refused.code, ErrorCode::PayloadTooLarge);
    assert_eq!(
        refused
            .params
            .get("maxBytes")
            .and_then(|value| value.as_u64()),
        Some(super::super::MAX_PLUGIN_TRANSFER_BYTES as u64)
    );
}
