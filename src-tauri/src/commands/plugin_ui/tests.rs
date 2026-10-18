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

/// The same plugin without a UI half — the manifest that makes "it has no window" the answer.
const HEADLESS_MANIFEST: &str = r#"{
    "id": "@synra-plugin/chat",
    "version": "1.0.0",
    "title": "Chat",
    "publisher": { "id": "synra-official" },
    "sdk": { "min": "1.0.0" },
    "wit": "synra:plugin@1.0.0",
    "runtime": { "headless": { "artifact": "dist/plugin.wasm", "stateful": false } },
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

/// The same plugin, installed, with a manifest that declares no UI.
fn installed_headless_only() -> (crate::testing::Root, crate::AppState) {
    let (root, state) = crate::testing::file_state();
    let directory = state.paths.plugin_dir(ID);
    std::fs::create_dir_all(&directory).expect("the plugin directory");
    std::fs::write(directory.join("manifest.json"), HEADLESS_MANIFEST).expect("a manifest");
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

    let entry = state
        .hosted_ui
        .lock()
        .get(&request.label)
        .cloned()
        .expect("the page is recorded");
    assert_eq!(entry.plugin_id.as_str(), ID);
    assert_eq!(entry.version, "1.0.0");
    assert!(!entry.ready, "it has not said it has drawn yet");
}

#[test]
fn opening_a_page_issues_the_grants_the_user_allowed() {
    // ★ The page's permissions are the decisions the user made, not an empty set. `open_request` used to issue with
    // `CapabilitySet::new()` — so every page was powerless no matter what it had been granted, and `env.can` would answer
    // false for a capability the user had approved.
    let (_root, state) = installed_with_ui();
    state
        .db()
        .record_grants(
            ID,
            &[("log:*".to_owned(), true), ("network:fetch".to_owned(), false)],
            crate::testing::FIXED_NOW_MS,
            "sha256-manifest",
        )
        .expect("decisions");
    let request = super::open_request(&state, ID).expect("it has a UI");
    let entry = state.hosted_ui.lock().get(&request.label).cloned().expect("recorded");
    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();

    let session = state
        .ui_sessions
        .validate(
            entry.nonce,
            &synra_protocol::ids::PluginId::parse(ID).expect("an id"),
            "1.0.0",
            now,
        )
        .expect("a live session");

    let allowed = synra_capability::Capability::parse("log:write").expect("a capability");
    let refused = synra_capability::Capability::parse("network:fetch").expect("a capability");
    assert!(session.grants().contains(&allowed), "a granted family did not reach the session");
    assert!(!session.grants().contains(&refused), "a refused capability reached the session");
}

#[test]
fn opening_a_page_for_a_plugin_that_is_not_installed_is_refused() {
    // ★ The refusal a caller can act on: "install it".
    let state = crate::testing::state();

    let refused = super::open_request(&state, "@acme/absent").expect_err("nothing is installed");

    assert_eq!(refused.code, ErrorCode::PluginNotInstalled);
    assert!(state.hosted_ui.lock().is_empty(), "and nothing was recorded");
}

#[test]
fn opening_a_page_for_a_plugin_with_no_ui_is_a_different_refusal() {
    // ★ The other refusal, and a different instruction: it is installed, so "install it" would be wrong advice. It has no
    // window to open, and nothing should be recorded or issued for a page that will never exist.
    let (_root, state) = installed_headless_only();

    let refused = super::open_request(&state, ID).expect_err("it has no UI");

    assert_eq!(refused.code, ErrorCode::Unsupported);
    assert!(state.hosted_ui.lock().is_empty(), "and no page was recorded");
    assert!(state.ui_sessions.is_empty(), "nor a session issued");
}

#[test]
fn a_plugin_address_splits_at_the_id_the_host_opened() {
    // ★ The reason this is not a URL parse: an id contains a slash, so `synra-plugin://@acme/hello/index.html` would split
    // into host `@acme` and path `/hello/index.html` — the id broken in the wrong place.
    let ids: Vec<synra_protocol::ids::PluginId> = ["@acme/hello", "@acme/hello-world"]
        .iter()
        .map(|id| synra_protocol::ids::PluginId::parse(id).expect("an id"))
        .collect();

    let (id, path) = super::split_ui_url("synra-plugin://@acme/hello/index.html", &ids).expect("it is one of ours");

    assert_eq!(id.as_str(), "@acme/hello");
    assert_eq!(path, "index.html");
}

#[test]
fn the_longest_id_wins_when_two_share_a_prefix() {
    // ★ First-match would let `@acme/hello` serve `@acme/hello-world`'s files, which is one plugin reading another's bundle.
    let ids: Vec<synra_protocol::ids::PluginId> = ["@acme/hello", "@acme/hello-world"]
        .iter()
        .map(|id| synra_protocol::ids::PluginId::parse(id).expect("an id"))
        .collect();

    let (id, path) = super::split_ui_url("synra-plugin://@acme/hello-world/app.js", &ids).expect("it is one of ours");

    assert_eq!(id.as_str(), "@acme/hello-world");
    assert_eq!(path, "app.js");
}

#[test]
fn an_address_for_an_unknown_plugin_is_not_split() {
    let ids = [synra_protocol::ids::PluginId::parse("@acme/hello").expect("an id")];

    for url in ["synra-plugin://@acme/other/index.html", "https://example.com/x", "synra-plugin://@acme/hello"] {
        assert!(super::split_ui_url(url, &ids).is_none(), "{url} must not be served");
    }
}

#[test]
fn a_bundle_file_is_served_with_its_type() {
    use std::path::Path;

    let root = std::env::temp_dir().join(format!("synra-ui-{}", std::process::id()));
    std::fs::create_dir_all(root.join("assets")).expect("a bundle");
    std::fs::write(root.join("index.html"), b"<!doctype html>").expect("a page");
    std::fs::write(root.join("assets/app.js"), b"export {}").expect("a script");

    let (bytes, content_type) = super::ui_file(&root, "index.html").expect("it is there");

    assert_eq!(content_type, "text/html; charset=utf-8");
    assert_eq!(bytes, b"<!doctype html>");
    let (_, script_type) = super::ui_file(&root, "assets/app.js").expect("it is there");
    assert!(script_type.starts_with("text/javascript"), "got {script_type}");

    let _ = std::fs::remove_dir_all(Path::new(&root));
}

#[test]
fn a_path_that_leaves_the_bundle_is_refused() {
    // ★ The assertion a security review asks for. `../../identity.toml` is the request that would hand a plugin the
    // device's private key, and both guards have to say no: the artifact rules, and the containment check after the join.
    let root = std::env::temp_dir().join(format!("synra-ui-escape-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("a bundle");

    for path in ["../identity.toml", "../../etc/passwd", "/etc/passwd"] {
        let refused = super::ui_file(&root, path).expect_err("{path} must not be served");
        assert_eq!(refused.code, synra_protocol::ErrorCode::InvalidArgument, "{path}");
    }

    let _ = std::fs::remove_dir_all(&root);
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

// ── The plugin page's call channel ───────────────────────────────────────────────────────────────

/// A page the host opened, and the token it was issued, in the decimal form the page presents.
fn hosted_page(state: &crate::AppState) -> String {
    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    let nonce = state.ui_sessions.issue_with(
        &plugin_id(),
        "1.0.0",
        CapabilitySet::new(),
        123_456_789_012_345_678_901_234_567_890_u128,
        now,
    );
    state.hosted_ui.lock().insert(
        "page-rpc".to_owned(),
        HostedUi { plugin_id: plugin_id(), version: "1.0.0".to_owned(), nonce, ready: false },
    );
    nonce.to_string()
}

#[test]
fn a_token_that_is_not_a_number_is_an_argument_error() {
    let state = crate::testing::state();

    let refused = super::plugin_ui_rpc(&state, "not-a-number", "device.list", &serde_json::json!({}))
        .expect_err("a token is a decimal number");

    assert_eq!(refused.code, ErrorCode::InvalidArgument);
}

#[test]
fn a_token_no_hosted_page_holds_is_refused() {
    // ★ The check that makes the token the credential: the state knows a page exists (another one), and this token is
    // still refused — so "some page exists" is not what is being checked.
    let state = crate::testing::state();
    let _other = hosted_page(&state);

    let refused = super::plugin_ui_rpc(&state, "42", "device.list", &serde_json::json!({}))
        .expect_err("that token belongs to no page");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
}

#[test]
fn an_unknown_method_is_refused_by_name() {
    let state = crate::testing::state();
    let token = hosted_page(&state);

    let refused = super::plugin_ui_rpc(&state, &token, "device.explode", &serde_json::json!({}))
        .expect_err("that is not an operation");

    assert_eq!(refused.code, ErrorCode::Unsupported);
    assert_eq!(refused.params.get("method").and_then(|value| value.as_str()), Some("device.explode"));
}

#[test]
fn a_method_on_the_surface_with_no_body_is_unsupported() {
    // ★ Addressable, not implemented. The alternative — an empty list or a resolved promise — would be a host that
    // claims to store something and does not.
    let state = crate::testing::state();
    let token = hosted_page(&state);

    let refused = super::plugin_ui_rpc(&state, &token, "storage.get", &serde_json::json!({"key": "x"}))
        .expect_err("storage is not built here");

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
            link_token: String::new(),
            // No address and no session: the card is offline, which is a field the plugin sees rather than a reason the
            // device is hidden.
            address: None,
            paired_at_ms: crate::testing::FIXED_NOW_MS,
        })
        .expect("a paired device");
    let token = hosted_page(&state);

    let answer = super::plugin_ui_rpc(&state, &token, "device.list", &serde_json::json!({}))
        .expect("a list");
    let devices = answer.as_array().expect("a bare array");

    assert_eq!(devices.len(), 1, "{answer}");
    assert_eq!(devices[0]["deviceId"], "11111111-1111-4111-8111-111111111111");
    assert_eq!(devices[0]["displayName"], "Laptop");
    assert_eq!(devices[0]["platform"], "linux");
    assert_eq!(devices[0]["online"], false);
    // ★ The reduced shape, not the host's card: a plugin may not receive an address seen on the network, a blocked flag
    // or a transfer summary.
    assert!(devices[0].get("address").is_none(), "the plugin received the host's card: {answer}");
    assert!(devices[0].get("transfer").is_none(), "the plugin received the host's card: {answer}");
}
