use super::*;

// ── Outbound network ────────────────────────────────────────────────────────────────────────────

/// A state with a plugin engine, so `network.fetch` has a host to ask.
fn network_page() -> (crate::testing::Root, crate::AppState, String) {
    let (root, state) = crate::testing::file_state();
    let host = Arc::new(crate::plugin_host::DesktopHost::with_platform(
        Arc::clone(&state.db),
        Arc::clone(&state.platform),
    ));
    let backend = Backend::new(Arc::new(ScriptedBackend::new()));
    let state = state.with_plugins(Arc::new(Mutex::new(synra_core::dispatch::PluginSet::new(
        backend, host,
    ))));
    let token = hosted_page(&state);
    (root, state, token)
}

#[test]
fn a_per_request_timeout_is_refused_rather_than_ignored() {
    // ★ Accepting `timeoutMs` and using the host's default would be a promise nothing keeps: a plugin that sets 500 ms
    // to bound a render would wait the client's own timeout and report it as a slow network.
    let state = crate::testing::state();
    let token = hosted_page(&state);

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "network.fetch",
        &serde_json::json!({"input": "https://example.com", "init": {"timeoutMs": 500}}),
    )
    .expect_err("per-request timeouts are not implemented");

    assert_eq!(refused.code, ErrorCode::Unsupported);
}

#[test]
fn a_byte_body_is_refused_before_the_request_is_made() {
    // The wire request carries a string; a `Uint8Array` cannot cross it, and silently dropping it would send a bodyless
    // request that the server answers differently.
    let state = crate::testing::state();
    let token = hosted_page(&state);

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "network.fetch",
        &serde_json::json!({"input": "https://example.com", "init": {"body": [1, 2, 3]}}),
    )
    .expect_err("a byte body cannot cross this channel");

    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(
        refused.params.get("field").and_then(|value| value.as_str()),
        Some("body")
    );
}

#[test]
fn a_shell_with_no_plugin_engine_cannot_fetch() {
    // The page can exist without the engine (it is a window over the kernel); the answer is the honest "this build has
    // no plugin engine", not a network error that sends the author to their own code.
    let state = crate::testing::state();
    let token = hosted_page(&state);

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "network.fetch",
        &serde_json::json!({"input": "https://example.com"}),
    )
    .expect_err("no engine");

    assert_eq!(refused.code, ErrorCode::Unsupported);
}

#[test]
fn a_plugin_without_a_network_grant_is_refused_by_the_host() {
    // ★ The authority is the host's, not this module's: `fetcher_for` reads `network:fetch:<host>` from the grants and
    // refuses an empty list rather than treating it as "any host".
    let (_root, state, token) = network_page();

    let refused = super::super::plugin_ui_rpc(
        &state,
        &token,
        "network.fetch",
        &serde_json::json!({"input": "https://example.com"}),
    )
    .expect_err("no network grant");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
}

#[test]
fn the_fetch_codes_reach_the_interface_unchanged() {
    // The host-call vocabulary and the protocol's are different spellings; a code that fell through to `internal` would
    // turn "this host is not allowed" into "something went wrong", which is the one answer a plugin cannot act on.
    let cases = [
        ("hostNotAllowed", ErrorCode::HostNotAllowed),
        ("badArgument", ErrorCode::InvalidArgument),
        ("responseTooLarge", ErrorCode::ResponseTooLarge),
        ("timeout", ErrorCode::Timeout),
        ("capability_denied", ErrorCode::CapabilityDenied),
        ("something-new", ErrorCode::Internal),
    ];

    for (wire, expected) in cases {
        let mapped = super::super::call_error(&CallError::new(wire, "a reason"));
        assert_eq!(mapped.code, expected, "{wire}");
        assert_eq!(
            mapped.params.get("reason").and_then(|value| value.as_str()),
            Some("a reason"),
            "the host's own message is lost: {wire}"
        );
    }
}
