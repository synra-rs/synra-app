use super::*;

#[test]
fn a_second_open_is_refused_without_a_second_token() {
    // ★ The check and the reservation happen under one lock: two fast clicks must not each issue a token, and the second
    // must not replace the first entry in `hosted_ui`.
    let (_root, state) = installed_with_ui();
    let _first = opened(&state, ID);
    let issued = state.ui_sessions.issued();

    let second = super::super::open_request(&state, ID).expect("a decision");

    assert!(matches!(second, OpenDecision::AlreadyOpen { .. }));
    assert_eq!(state.ui_sessions.issued(), issued, "no second token");
    assert_eq!(state.hosted_ui.lock().len(), 1);
}

#[test]
fn a_page_this_shell_never_hosted_is_refused() {
    let state = crate::testing::state();

    let refused = plugin_ui_ready(&state, "webview-does-not-exist").expect_err("nothing is hosted");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
}

#[test]
fn a_hosted_page_whose_token_is_unknown_is_refused() {
    // ★ The case that proves the check is real: the entry exists, so the label is not the reason for the refusal.
    let state = crate::testing::state();
    state.hosted_ui.lock().insert(
        "page-1".to_owned(),
        HostedUi {
            authority: test_authority(&state),
            plugin_id: plugin_id(),
            version: "1.0.0".to_owned(),
            nonce: 7,
            ready: false,
        },
    );

    let refused = plugin_ui_ready(&state, "page-1").expect_err("no such session");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
    let marked = state
        .hosted_ui
        .lock()
        .get("page-1")
        .map(|entry| entry.ready);
    assert_eq!(marked, Some(false), "and it is not marked ready");
}

#[test]
fn a_hosted_page_with_a_live_session_is_marked_ready() {
    let state = crate::testing::state();
    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    let nonce =
        state
            .ui_sessions
            .issue_placeholder(&plugin_id(), "1.0.0", CapabilitySet::new(), now);
    state.hosted_ui.lock().insert(
        "page-2".to_owned(),
        HostedUi {
            authority: test_authority(&state),
            plugin_id: plugin_id(),
            version: "1.0.0".to_owned(),
            nonce,
            ready: false,
        },
    );

    plugin_ui_ready(&state, "page-2").expect("a live session");

    let marked = state
        .hosted_ui
        .lock()
        .get("page-2")
        .map(|entry| entry.ready);
    assert_eq!(marked, Some(true));
}

#[test]
fn a_session_that_expired_is_refused() {
    // ★ A different refusal from "no such token": the registry knows this nonce and rejects it on age. Both are the same
    // answer to the caller (see the module docs), but a check that only ever sees unknown nonces could stop validating
    // expiry entirely and nothing here would notice.
    let state = crate::testing::state();
    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    let long_ago = now - synra_plugin_ui::SESSION_LIFETIME_MS - 1;
    let nonce =
        state
            .ui_sessions
            .issue_placeholder(&plugin_id(), "1.0.0", CapabilitySet::new(), long_ago);
    state.hosted_ui.lock().insert(
        "page-3".to_owned(),
        HostedUi {
            authority: test_authority(&state),
            plugin_id: plugin_id(),
            version: "1.0.0".to_owned(),
            nonce,
            ready: false,
        },
    );

    let refused = plugin_ui_ready(&state, "page-3").expect_err("the session is too old");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
    let marked = state
        .hosted_ui
        .lock()
        .get("page-3")
        .map(|entry| entry.ready);
    assert_eq!(marked, Some(false), "and it is not marked ready");
}

#[test]
fn a_scoped_plugin_gets_a_window_label_without_a_slash() {
    // ★ The label is what makes "one page per plugin" findable: the id's slash cannot appear in a window label, and the
    // mapping has to be the same one `open_request` recorded or the second open would build a second window.
    let label = super::super::window_label_prefix(&plugin_id());
    assert_eq!(label, super::super::origin::label(&plugin_id()));
    assert!(
        label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    );
    assert_ne!(
        label,
        super::super::window_label_prefix(&synra_protocol::PluginId::parse("@other/hello").unwrap())
    );
}

#[test]
fn closing_a_page_ends_its_session() {
    // ★ The token is the page's authority. An entry that outlives its window keeps answering for a page nobody can see,
    // and a second window with the same label would inherit the first one's nonce.
    let state = crate::testing::state();
    let token = hosted_page(&state);
    let nonce: u128 = token.parse().expect("the token is the nonce in decimal");

    super::super::forget_window(&state, "page-rpc");

    assert!(state.hosted_ui.lock().get("page-rpc").is_none());
    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    assert!(
        state
            .ui_sessions
            .validate(nonce, &plugin_id(), "1.0.0", now)
            .is_err(),
        "the session is gone"
    );
}
