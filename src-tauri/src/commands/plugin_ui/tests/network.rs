use super::*;

// ── Outbound network ────────────────────────────────────────────────────────────────────────────

/// A state with a plugin engine, so `network.fetch` has a host to ask.
fn network_page(grants: &[String]) -> (crate::testing::Root, Arc<crate::AppState>, String) {
    let (root, state) = crate::testing::file_state();
    test_authority(&state);
    let decisions: Vec<_> = grants.iter().cloned().map(|grant| (grant, true)).collect();
    state
        .db()
        .record_grants(plugin_id().as_str(), &decisions, 1, "scope")
        .unwrap();
    let host = Arc::new(crate::plugin_host::DesktopHost::with_platform(
        Arc::clone(&state.db),
        Arc::clone(&state.platform),
    ));
    let backend = Backend::new(Arc::new(ScriptedBackend::new()));
    let state = state.with_plugins(Arc::new(Mutex::new(synra_core::dispatch::PluginSet::new(
        backend, host,
    ))));
    let capabilities: Vec<_> = grants.iter().map(String::as_str).collect();
    let token = hosted_page_with(&state, &capabilities);
    (root, Arc::new(state), token)
}

#[test]
fn a_per_request_timeout_is_refused_rather_than_ignored() {
    // ★ Accepting `timeoutMs` and using the host's default would be a promise nothing keeps: a plugin that sets 500 ms
    // to bound a render would wait the client's own timeout and report it as a slow network.
    let state = std::sync::Arc::new(crate::testing::state());
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
    let state = std::sync::Arc::new(crate::testing::state());
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
    let state = std::sync::Arc::new(crate::testing::state());
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
    // The host checks the requested scope against stored grants.
    let (_root, state, token) = network_page(&[]);

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
fn network_scope_matches_the_shared_facade_and_testkit_cases() {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Scenario {
        name: String,
        input: String,
        method: Option<String>,
        grants: Vec<String>,
        code: Option<ErrorCode>,
        app_code: Option<ErrorCode>,
        capability: Option<String>,
    }

    let scenarios: Vec<Scenario> = serde_json::from_str(include_str!(
        "../../../../../tests/fixtures/network-scope.json"
    ))
    .expect("shared network scenarios");
    for scenario in scenarios {
        let (_root, state, token) = network_page(&scenario.grants);
        let mut args = serde_json::json!({"input": scenario.input});
        if let Some(method) = scenario.method {
            args["init"] = serde_json::json!({"method": method});
        }
        let refused = super::super::plugin_ui_rpc(&state, &token, "network.fetch", &args)
            .expect_err(&scenario.name);
        let wire = serde_json::to_value(refused).unwrap();
        let code = scenario.app_code.or(scenario.code).expect("host outcome");
        assert_eq!(
            wire["code"],
            serde_json::json!(code),
            "{}: {wire}",
            scenario.name
        );
        if let Some(capability) = scenario.capability {
            assert!(
                wire["params"]["reason"]
                    .as_str()
                    .unwrap()
                    .contains(&capability),
                "{wire}"
            );
        }
    }
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
