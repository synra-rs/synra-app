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

/// A plugin identity, and the manifest that gives it a UI half.
const ID: &str = "@synra-plugin/chat";
const UI_MANIFEST: &str = r#"{
    "id": "@synra-plugin/chat",
    "version": "1.0.0",
    "title": "Chat",
    "publisher": { "id": "synra-official" },
    "sdk": { "min": "1.0.0" },
    "wit": "synra:plugin@1.0.0",
    "runtime": {
        "headless": { "artifact": "dist/plugin.wasm", "stateful": false },
        "ui": { "artifact": "dist/ui.zip", "entry": "dist/index.html" }
    },
    "capabilities": [{ "cap": "log:*" }]
}"#;

/// A state with that plugin installed on disk.
fn installed_with_ui() -> (crate::testing::Root, crate::AppState) {
    let (root, state) = crate::testing::file_state();
    let directory = state.paths.plugin_dir(ID);
    std::fs::create_dir_all(&directory).expect("the plugin directory");
    std::fs::write(directory.join("manifest.json"), UI_MANIFEST).expect("a manifest");
    state
        .db()
        .insert_plugin(ID, true, false, "installed", crate::testing::FIXED_NOW_MS, crate::testing::FIXED_NOW_MS)
        .expect("a plugin row");
    (root, state)
}

#[test]
fn opening_a_page_records_who_it_belongs_to_and_where_it_lives() {
    // ★ The three values `shell.rs` hands to Tauri, and the table entry that makes `plugin_ui_ready` possible. The address
    // shape is the design's (`synra-plugin://<plugin-id>/<page>`); the page key falls back to the entry's stem because
    // `dist/index.html` is a path, not a page.
    let (_root, state) = installed_with_ui();

    let request = super::open_request(&state, ID).expect("it has a UI");

    assert!(request.url.starts_with("synra-plugin://@synra-plugin/chat/"), "got {}", request.url);
    assert!(request.url.ends_with("/index"), "the entry's stem is the page: {}", request.url);
    assert!(request.script.contains(&request.label), "the script names the page it is for");

    let hosted = state.hosted_ui.lock();
    let entry = hosted.get(&request.label).expect("the page is recorded");
    assert_eq!(entry.plugin_id.as_str(), ID);
    assert_eq!(entry.version, "1.0.0");
    assert!(!entry.ready, "it has not said it has drawn yet");
}

#[test]
fn opening_a_page_for_a_plugin_that_is_not_installed_is_refused() {
    // ★ The refusal a caller can act on: "install it". The other half — a plugin with no UI half — needs a fixture whose
    // manifest declares `runtime.ui`, and that fixture does not exist yet; this test covers the path a typo reaches.
    let state = crate::testing::state();

    let refused = super::open_request(&state, "@acme/absent").expect_err("nothing is installed");

    assert_eq!(refused.code, ErrorCode::PluginNotInstalled);
    assert!(state.hosted_ui.lock().is_empty(), "and nothing was recorded");
}

#[test]
fn the_initialization_script_carries_the_label_and_a_string_token() {
    // ★ The token is quoted, and that is the assertion worth having: `u128::MAX` is not representable as a JavaScript
    // number, and a script that emitted it bare would round — the page would then present a token that validates for
    // nobody, and the symptom would look like "the page never became ready".
    let script = super::initialization_script("page-1", u128::MAX);

    assert!(script.contains("\"page-1\""), "the label, quoted");
    assert!(
        script.contains(&format!("{:?}", u128::MAX.to_string())),
        "the token as a **string**, so no digits are lost"
    );
    assert!(script.starts_with("Object.freeze("), "the page gets a frozen object, not a mutable global");
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
