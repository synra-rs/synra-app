//! What `plugin_ui_ready` accepts and refuses.
//!
//! The cases worth asserting are the two refusals and one acceptance: a label this shell never hosted, a hosted page whose
//! token the registry does not know, and a hosted page holding a live session.

use super::plugin_ui_ready;
use crate::state::HostedUi;
use synra_capability::CapabilitySet;
use synra_protocol::ErrorCode;

/// A plausible plugin id, so nothing here accidentally tests id parsing.
fn plugin_id() -> synra_protocol::ids::PluginId {
    synra_protocol::ids::PluginId::parse("@acme/hello").expect("a plugin id")
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
        HostedUi { plugin_id: plugin_id(), version: "1.0.0".to_owned(), nonce: 7, ready: false },
    );

    let refused = plugin_ui_ready(&state, "page-1").expect_err("no such session");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
    let marked = state.hosted_ui.lock().get("page-1").map(|entry| entry.ready);
    assert_eq!(marked, Some(false), "and it is not marked ready");
}

#[test]
fn a_hosted_page_with_a_live_session_is_marked_ready() {
    let state = crate::testing::state();
    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    let nonce = state
        .ui_sessions
        .issue_placeholder(&plugin_id(), "1.0.0", CapabilitySet::new(), now);
    state.hosted_ui.lock().insert(
        "page-2".to_owned(),
        HostedUi { plugin_id: plugin_id(), version: "1.0.0".to_owned(), nonce, ready: false },
    );

    plugin_ui_ready(&state, "page-2").expect("a live session");

    let marked = state.hosted_ui.lock().get("page-2").map(|entry| entry.ready);
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
    let nonce = state
        .ui_sessions
        .issue_placeholder(&plugin_id(), "1.0.0", CapabilitySet::new(), long_ago);
    state.hosted_ui.lock().insert(
        "page-3".to_owned(),
        HostedUi { plugin_id: plugin_id(), version: "1.0.0".to_owned(), nonce, ready: false },
    );

    let refused = plugin_ui_ready(&state, "page-3").expect_err("the session is too old");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
    let marked = state.hosted_ui.lock().get("page-3").map(|entry| entry.ready);
    assert_eq!(marked, Some(false), "and it is not marked ready");
}
