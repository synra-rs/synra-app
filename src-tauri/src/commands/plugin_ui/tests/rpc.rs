use super::*;

#[test]
fn a_token_that_is_not_a_number_is_an_argument_error() {
    let state = crate::testing::state();

    let refused = super::super::plugin_ui_rpc(
        &state,
        "not-a-number",
        "device.list",
        &serde_json::json!({}),
    )
    .expect_err("a token is a decimal number");

    assert_eq!(refused.code, ErrorCode::InvalidArgument);
}

#[test]
fn a_token_no_hosted_page_holds_is_refused() {
    // ★ The check that makes the token the credential: the state knows a page exists (another one), and this token is
    // still refused — so "some page exists" is not what is being checked.
    let state = crate::testing::state();
    let _other = hosted_page(&state);

    let refused = super::super::plugin_ui_rpc(&state, "42", "device.list", &serde_json::json!({}))
        .expect_err("that token belongs to no page");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
}

#[test]
fn an_unknown_method_is_refused_by_name() {
    let state = crate::testing::state();
    let token = hosted_page(&state);

    let refused = super::super::plugin_ui_rpc(&state, &token, "device.explode", &serde_json::json!({}))
        .expect_err("that is not an operation");

    assert_eq!(refused.code, ErrorCode::Unsupported);
    assert_eq!(
        refused
            .params
            .get("method")
            .and_then(|value| value.as_str()),
        Some("device.explode")
    );
}

#[test]
fn a_method_on_the_surface_with_no_body_is_unsupported() {
    // ★ Addressable, not implemented. The alternative — an empty list or a resolved promise — would be a host that
    // claims to fetch something and does not.
    let state = crate::testing::state();
    let token = hosted_page(&state);

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "network.fetch",
        &serde_json::json!({"input": "https://example.invalid"}),
    )
    .expect_err("network fetch is not built here");

    assert_eq!(refused.code, ErrorCode::Unsupported);
}

#[test]
fn device_list_answers_the_four_fields_a_plugin_may_see() {
    let (_root, state) = crate::testing::file_state();
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: "11111111-1111-4111-8111-111111111111".to_owned(),
            display_name: "Laptop".to_owned(),
            platform: "linux".to_owned(),
            features: "[]".to_owned(),
            // No address and no session: the card is offline, which is a field the plugin sees rather than a reason the
            // device is hidden.
            address: None,
            paired_at_ms: crate::testing::FIXED_NOW_MS,
        })
        .expect("a paired device");
    state
        .db()
        .record_seen_device(&synra_store::Seen {
            device_id: "22222222-2222-4222-8222-222222222222".to_owned(),
            display_name: "Unpaired discovery".to_owned(),
            platform: "windows".to_owned(),
            seen_at_ms: crate::testing::FIXED_NOW_MS,
            source: "udp".to_owned(),
        })
        .unwrap();
    let token = hosted_page_with(&state, &["device:list"]);

    let answer = super::super::plugin_ui_rpc(&state, &token, "device.list", &serde_json::json!({}))
        .expect("a list");
    let devices = answer.as_array().expect("a bare array");

    assert_eq!(devices.len(), 1, "{answer}");
    assert_eq!(
        devices[0]["deviceId"],
        "11111111-1111-4111-8111-111111111111"
    );
    assert_eq!(devices[0]["displayName"], "Laptop");
    assert_eq!(devices[0]["platform"], "linux");
    assert_eq!(devices[0]["online"], false);
    // ★ The reduced shape, not the host's card: a plugin may not receive an address seen on the network, a blocked flag
    // or a transfer summary.
    assert!(
        devices[0].get("address").is_none(),
        "the plugin received the host's card: {answer}"
    );
    assert!(
        devices[0].get("transfer").is_none(),
        "the plugin received the host's card: {answer}"
    );
}

#[test]
fn device_list_requires_its_current_page_grant() {
    let (_root, state) = crate::testing::file_state();
    let token = hosted_page(&state);
    let error = super::super::plugin_ui_rpc(&state, &token, "device.list", &serde_json::json!({}))
        .expect_err("a valid page is not a device-list grant");
    assert_eq!(error.code, ErrorCode::CapabilityDenied);
}

#[test]
fn device_list_refuses_an_oversized_reply_instead_of_silently_dropping_targets() {
    let (_root, state) = crate::testing::file_state();
    for index in 1..=257 {
        state
            .db()
            .pair_device(&synra_store::Paired {
                device_id: format!("00000000-0000-4000-8000-{index:012x}"),
                display_name: format!("PC {index}"),
                platform: "windows".to_owned(),
                features: "[]".to_owned(),
                address: None,
                paired_at_ms: crate::testing::FIXED_NOW_MS,
            })
            .unwrap();
    }
    let token = hosted_page_with(&state, &["device:list"]);
    let error = super::super::plugin_ui_rpc(&state, &token, "device.list", &serde_json::json!({}))
        .expect_err("a truncated list must not appear complete");
    assert_eq!(error.code, ErrorCode::PayloadTooLarge);
}
