//! What `plugin_ui_ready` accepts and refuses.
//!
//! The cases worth asserting are the two refusals and one acceptance: a label this shell never hosted, a hosted page whose
//! token the registry does not know, and a hosted page holding a live session.

use super::{OpenDecision, OpenRequest, plugin_ui_ready};
use crate::state::HostedUi;
use parking_lot::Mutex;
use std::sync::Arc;
use synra_capability::CapabilitySet;
use synra_platform::{CommandPlatform, CommandRunner, Desktop, PlatformError};
use synra_plugin_host::host_calls::CallError;
use synra_plugin_host::{Backend, ScriptedBackend};
use synra_protocol::ErrorCode;

mod ui_authority;
mod wasm_errors;

#[cfg(feature = "tauri-shell")]
mod native_acl;

#[cfg(feature = "official-plugin-tests")]
mod official_starter;

/// A plausible plugin id, so nothing here accidentally tests id parsing.
fn plugin_id() -> synra_protocol::ids::PluginId {
    synra_protocol::ids::PluginId::parse("@acme/hello").expect("a plugin id")
}

/// Give manually hosted fixture pages real persistent eligibility.
fn test_authority(state: &crate::AppState) -> synra_store::PluginAuthority {
    let db = state.db();
    if db.plugin_authority(plugin_id().as_str()).unwrap().is_none() {
        db.insert_plugin(plugin_id().as_str(), true, true, "installed", 1, 1)
            .unwrap();
    }
    db.plugin_authority(plugin_id().as_str()).unwrap().unwrap()
}

/// Opens a page that must not be open, as a test's setup.
fn opened(state: &crate::AppState, plugin_id: &str) -> OpenRequest {
    match super::open_request(state, plugin_id).expect("a decision") {
        OpenDecision::Open(request) => request,
        OpenDecision::AlreadyOpen { .. } => panic!("the page was already open"),
    }
}

#[test]
fn a_second_open_is_refused_without_a_second_token() {
    // ★ The check and the reservation happen under one lock: two fast clicks must not each issue a token, and the second
    // must not replace the first entry in `hosted_ui`.
    let (_root, state) = installed_with_ui();
    let _first = opened(&state, ID);
    let issued = state.ui_sessions.issued();

    let second = super::open_request(&state, ID).expect("a decision");

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
        HostedUi {
            authority: test_authority(&state),
            plugin_id: plugin_id(),
            version: "1.0.0".to_owned(),
            nonce,
            ready: false,
        },
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
        .insert_plugin(
            ID,
            true,
            true,
            "installed",
            crate::testing::FIXED_NOW_MS,
            crate::testing::FIXED_NOW_MS,
        )
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
        .insert_plugin(
            ID,
            true,
            true,
            "installed",
            crate::testing::FIXED_NOW_MS,
            crate::testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");
    (root, state)
}

#[test]
fn opening_a_page_records_who_it_belongs_to_and_where_it_lives() {
    // Tauri must receive a parseable origin and the actual entry file, including its nested directory.
    let (_root, state) = installed_with_ui();

    let request = opened(&state, ID);

    let parsed = url::Url::parse(&request.url).expect("a real URL");
    assert_eq!(parsed.as_str(), request.url);
    assert_eq!(parsed.path(), "/dist/index.html");
    assert_eq!(
        super::split_ui_url(
            parsed.as_str(),
            &[synra_protocol::PluginId::parse(ID).unwrap()]
        )
        .unwrap()
        .1,
        "dist/index.html"
    );
    assert!(
        request.script.contains(&request.label),
        "the script names the page it is for"
    );

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
    let request = opened(&state, ID);
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
    // Scoped package names retain their complete identity after standard URL parsing.
    let ids: Vec<synra_protocol::ids::PluginId> = ["@acme/hello", "@acme/hello-world"]
        .iter()
        .map(|id| synra_protocol::ids::PluginId::parse(id).expect("an id"))
        .collect();

    let (id, path) = super::split_ui_url(
        &super::origin::entry(&ids[0], "index.html", None).unwrap(),
        &ids,
    )
    .expect("it is one of ours");

    assert_eq!(id.as_str(), "@acme/hello");
    assert_eq!(path, "index.html");
}

#[test]
fn packages_with_a_shared_name_prefix_have_separate_origins() {
    // A package name prefix never grants access to another package's files.
    let ids: Vec<synra_protocol::ids::PluginId> = ["@acme/hello", "@acme/hello-world"]
        .iter()
        .map(|id| synra_protocol::ids::PluginId::parse(id).expect("an id"))
        .collect();

    let (id, path) = super::split_ui_url(
        &super::origin::entry(&ids[1], "app.js", None).unwrap(),
        &ids,
    )
    .expect("it is one of ours");

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
fn the_initialization_script_keeps_the_seed_in_the_wrapper_and_calls_the_facade() {
    // ★ Three properties in one string:
    //   * the token is quoted, because `u128::MAX` is not representable as a JavaScript number and a bare one would
    //     round — the page would present a token that validates for nobody;
    //   * the seed is the **wrapper function's argument**, not a global, so a script on the page cannot read the token
    //     or rewrite its own grants;
    //   * the facade is inline rather than imported, because an initialization script has no module loader.
    let seed = super::InjectionSeed {
        label: "page-1",
        token: u128::MAX.to_string(),
        plugin_id: "@acme/hello",
        granted: &["log:*".to_owned()],
        denied_capabilities: &[],
        locale: "zh-CN",
        dark_mode: true,
        sdk_version: "0.1.0",
        platform: "linux",
        host_kind: "desktop",
        app_version: "9.9.9",
        platform_capabilities: vec!["open_url".to_owned()],
    };

    let script = super::initialization_script("/* facade */", &seed).expect("a script");

    assert!(
        script.starts_with("(function(seed){"),
        "the seed is a parameter of the wrapper: {script}"
    );
    assert!(script.contains("\"label\":\"page-1\""), "the label, quoted");
    assert!(
        script.contains(&format!("\"token\":{:?}", u128::MAX.to_string())),
        "the token as a **string**, so no digits are lost"
    );
    assert!(script.contains("\"granted\":[\"log:*\"]"), "the decisions travel with the seed");
    assert!(
        script.contains("\"platformCapabilities\":[\"open_url\"]"),
        "the platform's answer travels with the seed: {script}"
    );
    assert!(script.contains("/* facade */"), "the facade is inline");
    assert!(script.contains("SynraInjection.boot(seed);"), "and it is called with the seed");
    assert!(
        !script.contains("__SYNRA_PLUGIN__"),
        "the seed must not be reachable through a global: {script}"
    );
}

#[test]
fn opening_a_page_writes_the_environment_the_facade_answers_from() {
    // ★ The facts a plugin asks synchronously — platform, host kind, SDK version, locale, dark mode — are decided here,
    // before the page loads. A missing one would leave a namespace answering `undefined`, which is the failure class the
    // injected-surface tests exist to prevent.
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
    // The settings the user chose.
    super::super::config::config_set(
        &state,
        &synra_ipc::ConfigPatch {
            set: [
                ("ui.locale".to_owned(), serde_json::json!("zh-CN")),
                ("ui.theme".to_owned(), serde_json::json!("dark")),
            ]
            .into_iter()
            .collect(),
            unset: Vec::new(),
        },
    )
    .expect("settings");

    let request = opened(&state, ID);

    for expected in [
        "\"pluginId\":\"@synra-plugin/chat\"",
        "\"granted\":[\"log:*\"]",
        "\"deniedCapabilities\":[\"network:fetch\"]",
        "\"locale\":\"zh-CN\"",
        "\"darkMode\":true",
        "\"hostKind\":\"desktop\"",
        "\"appVersion\":\"",
        // ★ The platform's own answer, not a constant: `supported_actions` decides it, and on a desktop it always
        // includes opening a URL.
        "\"platformCapabilities\":[",
        "\"open_url\"",
    ] {
        assert!(request.script.contains(expected), "the seed is missing {expected}: {}", request.script);
    }
    assert!(request.script.contains(&format!(
        "\"sdkVersion\":\"{}\"",
        synra_plugin_host::SDK_VERSION
    )));
    // ★ And the façade itself is in the script, called with the seed. Without it the page would have a seed and no
    // client — which is the state this whole round exists to end.
    assert!(request.script.contains("SynraInjection.boot(seed);"), "the façade is called with the seed");
    assert!(
        request.script.contains("window.synra"),
        "the façade publishes the client: {}",
        &request.script[..request.script.len().min(200)]
    );
    assert!(
        !request.script.contains("__SYNRA_PLUGIN__"),
        "the token must not be reachable through a global"
    );
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
    let marked = state.hosted_ui.lock().get("page-3").map(|entry| entry.ready);
    assert_eq!(marked, Some(false), "and it is not marked ready");
}

// ── The plugin page's call channel ───────────────────────────────────────────────────────────────

/// A page the host opened, and the token it was issued, in the decimal form the page presents.
fn hosted_page(state: &crate::AppState) -> String {
    hosted_page_with(state, &[])
}

/// The same page, holding these capabilities.
fn hosted_page_with(state: &crate::AppState, grants: &[&str]) -> String {
    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    let capabilities = CapabilitySet::parse_all(grants.iter().copied()).expect("capabilities");
    let nonce = state.ui_sessions.issue_with(
        &plugin_id(),
        "1.0.0",
        capabilities,
        123_456_789_012_345_678_901_234_567_890_u128,
        now,
    );
    state.hosted_ui.lock().insert(
        "page-rpc".to_owned(),
        HostedUi {
            authority: test_authority(state),
            plugin_id: plugin_id(),
            version: "1.0.0".to_owned(),
            nonce,
            ready: false,
        },
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
    // claims to fetch something and does not.
    let state = crate::testing::state();
    let token = hosted_page(&state);

    let refused = super::plugin_ui_rpc(
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
            link_token: String::new(),
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

#[test]
fn device_list_requires_its_current_page_grant() {
    let (_root, state) = crate::testing::file_state();
    let token = hosted_page(&state);
    let error = super::plugin_ui_rpc(&state, &token, "device.list", &serde_json::json!({}))
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
                link_token: String::new(),
                address: None,
                paired_at_ms: crate::testing::FIXED_NOW_MS,
            })
            .unwrap();
    }
    let token = hosted_page_with(&state, &["device:list"]);
    let error = super::plugin_ui_rpc(&state, &token, "device.list", &serde_json::json!({}))
        .expect_err("a truncated list must not appear complete");
    assert_eq!(error.code, ErrorCode::PayloadTooLarge);
}

// ── Storage ──────────────────────────────────────────────────────────────────────────────────────

/// A state with the plugin installed (the storage table's foreign key needs the row) and a page holding both storage
/// capabilities.
fn storage_page() -> (crate::testing::Root, crate::AppState, String) {
    let (root, state) = crate::testing::file_state();
    state
        .db()
        .insert_plugin(plugin_id().as_str(), true, true, "installed", crate::testing::FIXED_NOW_MS, crate::testing::FIXED_NOW_MS)
        .expect("a plugin row");
    let token = hosted_page_with(&state, &["storage:local:read", "storage:local:write"]);
    (root, state, token)
}

#[test]
fn storage_round_trips_through_the_channel() {
    let (_root, state, token) = storage_page();

    let written = super::plugin_ui_rpc(
        &state,
        &token,
        "storage.set",
        &serde_json::json!({"key": "draft", "value": "hello"}),
    )
    .expect("a write");
    assert_eq!(written, serde_json::Value::Null);

    assert_eq!(
        super::plugin_ui_rpc(&state, &token, "storage.get", &serde_json::json!({"key": "draft"}))
            .expect("a read"),
        serde_json::json!("hello")
    );
    // An absent key is `null`, the client's `string | null` — not an error and not an empty string.
    assert_eq!(
        super::plugin_ui_rpc(&state, &token, "storage.get", &serde_json::json!({"key": "absent"}))
            .expect("a read"),
        serde_json::Value::Null
    );
    assert_eq!(
        super::plugin_ui_rpc(&state, &token, "storage.keys", &serde_json::json!({})).expect("keys"),
        serde_json::json!(["draft"])
    );
    assert_eq!(
        super::plugin_ui_rpc(&state, &token, "storage.quota", &serde_json::json!({})).expect("quota"),
        serde_json::json!({"usedBytes": 5, "maxBytes": 64 * 1024 * 1024})
    );

    assert_eq!(
        super::plugin_ui_rpc(&state, &token, "storage.delete", &serde_json::json!({"key": "draft"}))
            .expect("a delete"),
        serde_json::json!(true)
    );
    assert_eq!(
        super::plugin_ui_rpc(&state, &token, "storage.get", &serde_json::json!({"key": "draft"}))
            .expect("a read"),
        serde_json::Value::Null
    );
}

#[test]
fn a_storage_call_without_the_capability_is_refused() {
    // ★ The gate, not the façade's `env.can`: a page that somehow called the method directly still reaches this check.
    let (_root, state) = crate::testing::file_state();
    state
        .db()
        .insert_plugin(plugin_id().as_str(), true, true, "installed", crate::testing::FIXED_NOW_MS, crate::testing::FIXED_NOW_MS)
        .expect("a plugin row");
    let read_only = hosted_page_with(&state, &["storage:local:read"]);

    let refused = super::plugin_ui_rpc(
        &state,
        &read_only,
        "storage.set",
        &serde_json::json!({"key": "k", "value": "v"}),
    )
    .expect_err("no write capability");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
    assert_eq!(
        refused.params.get("capability").and_then(|value| value.as_str()),
        Some("storage:local:write")
    );
    assert_eq!(
        state.db().storage_get(plugin_id().as_str(), "k").expect("a read"),
        None,
        "a refused write left something behind"
    );
}

#[test]
fn a_key_that_is_not_a_string_is_an_argument_error() {
    let (_root, state, token) = storage_page();

    let refused = super::plugin_ui_rpc(&state, &token, "storage.get", &serde_json::json!({"key": 7}))
        .expect_err("a key is a string");

    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(refused.params.get("field").and_then(|value| value.as_str()), Some("key"));
}

#[test]
fn a_write_replaces_rather_than_double_counting_the_old_value() {
    // ★ The arithmetic a real 64 MiB ceiling cannot exercise: rewriting a key with a slightly larger value must not
    // count the value it replaces twice.
    assert_eq!(super::storage_after(10, 4, 6), 12);
    assert_eq!(super::storage_after(10, 8, 3), 5, "a smaller replacement shrinks the store");
    assert_eq!(super::storage_after(0, 0, 0), 0);
    // A stored value larger than the measured usage cannot happen through the store, but saturating keeps a corrupt
    // measurement from wrapping into a tiny number and letting an oversized write through.
    assert_eq!(super::storage_after(2, 9, 5), 5);
}

// ── The desktop: clipboard, browser, notifications ──────────────────────────────────────────────

/// Every program the fake runner was asked to launch, with its arguments.
type Calls = Arc<Mutex<Vec<(String, Vec<String>, Option<String>)>>>;

/// A platform whose programs are recorded instead of launched.
///
/// ★ Why the tests need one: every platform action ends in a program on this machine — `powershell` for the clipboard on
/// Windows, `pbcopy` on macOS, `notify-send` on Linux — and a test that really ran them would open a browser, overwrite
/// the user's clipboard or show a notification. The production code is unchanged; only the runner under it is.
#[derive(Clone)]
struct RecordingRunner {
    calls: Calls,
    answer: String,
}

impl CommandRunner for RecordingRunner {
    fn is_available(&self) -> bool {
        true
    }
    fn run(
        &self,
        program: &str,
        arguments: &[&str],
        input: Option<&str>,
    ) -> Result<String, PlatformError> {
        self.calls.lock().push((
            program.to_owned(),
            arguments.iter().map(|argument| (*argument).to_owned()).collect(),
            input.map(str::to_owned),
        ));
        Ok(self.answer.clone())
    }
}

/// A state whose platform records instead of acting, and a page holding these grants.
fn platform_page(grants: &[&str]) -> (crate::testing::Root, crate::AppState, String, RecordingRunner) {
    let (root, mut state) = crate::testing::file_state();
    let answer = if cfg!(windows) {
        serde_json::to_string("from the clipboard").expect("nullable text JSON")
    } else {
        "from the clipboard".to_owned()
    };
    let runner = RecordingRunner {
        calls: Arc::new(Mutex::new(Vec::new())),
        answer,
    };
    state.platform = Arc::new(CommandPlatform::with(
        Desktop::current(),
        Box::new(runner.clone()),
    ));
    let token = hosted_page_with(&state, grants);
    (root, state, token, runner)
}

#[test]
fn reading_the_clipboard_answers_with_the_platforms_text() {
    let (_root, state, token, runner) =
        platform_page(&["platform:read-clipboard"]);

    let answer = super::plugin_ui_rpc(&state, &token, "clipboard.read", &serde_json::json!({}))
        .expect("the clipboard");

    assert_eq!(answer, serde_json::json!("from the clipboard"));
    assert_eq!(runner.calls.lock().len(), 1, "the platform was asked exactly once");
}

#[test]
fn writing_the_clipboard_without_the_write_grant_never_reaches_the_platform() {
    // ★ The gate, not the façade: the `denied` a page sees from `synra.clipboard.write` is an optimisation; the check
    // that matters is the one between a call and the user's clipboard.
    let (_root, state, token, runner) = platform_page(&["platform:read-clipboard"]);

    let refused = super::plugin_ui_rpc(
        &state,
        &token,
        "clipboard.write",
        &serde_json::json!({"text": "x"}),
    )
    .expect_err("no write capability");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
    assert_eq!(
        refused.params.get("capability").and_then(|value| value.as_str()),
        Some("platform:write-clipboard")
    );
    assert!(runner.calls.lock().is_empty(), "a refused write launched a program");
}

#[test]
fn clipboard_write_delivers_text_to_the_process_stdin() {
    let (_root, state, token, runner) = platform_page(&["platform:write-clipboard"]);
    let text = "\"literal\"; $(1+2)\r\n中文🖱️";
    let answer = super::plugin_ui_rpc(
        &state,
        &token,
        "clipboard.write",
        &serde_json::json!({"text": text}),
    )
    .expect("clipboard write");
    assert_eq!(answer, serde_json::Value::Null);
    assert_eq!(runner.calls.lock().len(), 1);
    let call = runner.calls.lock().first().cloned().expect("one command");
    assert_eq!(call.2.as_deref(), Some(text));
    assert!(!call.1.join(" ").contains(text));
}

#[test]
fn oversized_clipboard_utf8_is_refused_before_a_process_starts() {
    let (_root, state, token, runner) = platform_page(&["platform:write-clipboard"]);
    let text = "选".repeat(synra_platform::MAX_CLIPBOARD_BYTES / 3 + 1);
    let error = super::plugin_ui_rpc(
        &state,
        &token,
        "clipboard.write",
        &serde_json::json!({"text": text}),
    )
    .expect_err("byte limit");
    assert_eq!(error.code, ErrorCode::PayloadTooLarge);
    assert!(runner.calls.lock().is_empty());
}

#[test]
fn opening_a_url_hands_it_to_the_platform() {
    let (_root, state, token, runner) = platform_page(&["platform:open-url"]);

    let answer = super::plugin_ui_rpc(
        &state,
        &token,
        "platform.openUrl",
        &serde_json::json!({"url": "https://example.com/page"}),
    )
    .expect("a URL");

    assert_eq!(answer, serde_json::Value::Null);
    // The guard is scoped: `calls` protects a `Vec` that nothing else in this test needs after the assertion, and
    // holding it across the rest of the test is what `significant_drop_tightening` refuses.
    let reached_url = {
        let calls = runner.calls.lock();
        assert_eq!(calls.len(), 1, "the opener was launched once");
        calls[0].1.iter().any(|argument| argument == "https://example.com/page")
    };
    assert!(reached_url, "the URL did not reach the platform");
}

#[test]
fn a_url_the_platform_refuses_never_reaches_the_runner() {
    let (_root, state, token, runner) = platform_page(&["platform:open-url"]);

    let refused = super::plugin_ui_rpc(
        &state,
        &token,
        "platform.openUrl",
        &serde_json::json!({"url": "file:///etc/passwd"}),
    )
    .expect_err("a local scheme is not opened");

    // The refusal is about the argument, and it names the field — a user can act on that; "internal" would send them to
    // the logs for a URL the policy already explained.
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(refused.params.get("field").and_then(|value| value.as_str()), Some("url"));
    assert!(runner.calls.lock().is_empty(), "a refused URL was opened anyway");
}

#[test]
fn a_notification_carries_its_title_and_body() {
    let (_root, state, token, runner) = platform_page(&["platform:notify"]);

    super::plugin_ui_rpc(
        &state,
        &token,
        "platform.notify",
        &serde_json::json!({"title": "Build", "body": "it finished"}),
    )
    .expect("a notification");

    let joined = {
        let calls = runner.calls.lock();
        assert_eq!(calls.len(), 1, "the notifier was launched once");
        if cfg!(windows) {
            calls[0].2.clone().expect("notification stdin")
        } else {
            calls[0].1.join(" ")
        }
    };
    assert!(joined.contains("Build"), "the title did not reach the notifier: {joined}");
    assert!(joined.contains("it finished"), "the body did not reach the notifier: {joined}");
}

// ── Outbound network ────────────────────────────────────────────────────────────────────────────

/// A state with a plugin engine, so `network.fetch` has a host to ask.
fn network_page() -> (crate::testing::Root, crate::AppState, String) {
    let (root, state) = crate::testing::file_state();
    let host = Arc::new(crate::plugin_host::DesktopHost::with_platform(
        Arc::clone(&state.db),
        Arc::clone(&state.platform),
    ));
    let backend = Backend::new(Arc::new(ScriptedBackend::new()));
    let state = state.with_plugins(Arc::new(Mutex::new(
        synra_core::dispatch::PluginSet::new(backend, host),
    )));
    let token = hosted_page(&state);
    (root, state, token)
}

#[test]
fn a_per_request_timeout_is_refused_rather_than_ignored() {
    // ★ Accepting `timeoutMs` and using the host's default would be a promise nothing keeps: a plugin that sets 500 ms
    // to bound a render would wait the client's own timeout and report it as a slow network.
    let state = crate::testing::state();
    let token = hosted_page(&state);

    let refused = super::plugin_ui_rpc(
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

    let refused = super::plugin_ui_rpc(
        &state,
        &token,
        "network.fetch",
        &serde_json::json!({"input": "https://example.com", "init": {"body": [1, 2, 3]}}),
    )
    .expect_err("a byte body cannot cross this channel");

    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(refused.params.get("field").and_then(|value| value.as_str()), Some("body"));
}

#[test]
fn a_shell_with_no_plugin_engine_cannot_fetch() {
    // The page can exist without the engine (it is a window over the kernel); the answer is the honest "this build has
    // no plugin engine", not a network error that sends the author to their own code.
    let state = crate::testing::state();
    let token = hosted_page(&state);

    let refused = super::plugin_ui_rpc(
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

    let refused = super::plugin_ui_rpc(
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
        let mapped = super::call_error(&CallError::new(wire, "a reason"));
        assert_eq!(mapped.code, expected, "{wire}");
        assert_eq!(
            mapped.params.get("reason").and_then(|value| value.as_str()),
            Some("a reason"),
            "the host's own message is lost: {wire}"
        );
    }
}

#[test]
fn a_platform_action_is_named_the_way_the_client_names_it() {
    // ★ Two vocabularies, on purpose: `platform:read-clipboard` is the permission the manifest declares, and
    // `clipboard_read` is what `env.platformSupports` takes. Passing one where the other belongs would answer `false`
    // for every action on a host that supports all of them — the exact bug this mapping exists to prevent.
    use synra_platform::PlatformAction as Action;

    assert_eq!(super::platform_capability(Action::OpenUrl), Some("open_url"));
    assert_eq!(super::platform_capability(Action::OpenPath), Some("open_path"));
    assert_eq!(super::platform_capability(Action::RevealPath), Some("reveal_path"));
    assert_eq!(super::platform_capability(Action::SimulateKeys), Some("simulate_keys"));
    assert_eq!(super::platform_capability(Action::ReadClipboard), Some("clipboard_read"));
    assert_eq!(super::platform_capability(Action::WriteClipboard), Some("clipboard_write"));
    assert_eq!(super::platform_capability(Action::Notify), Some("notify"));
}

#[test]
fn a_page_may_only_stay_on_itself() {
    // ★ `has_ui` says the plugin has a page; this says what the page may do with the window. A fragment is the same
    // document and is how a single-page plugin routes; every other destination changes what is shown while keeping the
    // token the window was opened with.
    let opened = "synra-plugin://acme/hello/index";

    assert!(super::stays_on_page(opened, opened));
    assert!(super::stays_on_page("synra-plugin://acme/hello/index#/settings", opened));
    assert!(super::stays_on_page("synra-plugin://acme/hello/index#", opened));

    // Another page of the same bundle is still a navigation, and would keep this page's authority.
    assert!(!super::stays_on_page("synra-plugin://acme/hello/other", opened));
    // The prefix trap: a page whose address merely starts with the opened one.
    assert!(!super::stays_on_page("synra-plugin://acme/hello/index2", opened));
    // Another plugin's origin, and the schemes a page might use to leave the app entirely.
    assert!(!super::stays_on_page("synra-plugin://acme/other/index", opened));
    assert!(!super::stays_on_page("https://example.com/", opened));
    assert!(!super::stays_on_page("file:///etc/passwd", opened));
    assert!(!super::stays_on_page("data:text/html,hello", opened));
    assert!(!super::stays_on_page("", opened));
}

// ── The plugin's own actions ────────────────────────────────────────────────────────────────────

/// A state whose plugin is loaded, has declared one action, and whose backend answers `actions.invoke` with `answer`.
///
/// The script is keyed by the call's **payload**, so `args_json` must be what `self.invoke` will serialise: an action
/// asked for without arguments arrives as `null`, not as `{}`.
fn action_page(answer: synra_plugin_host::Scripted, args_json: &str)
    -> (crate::testing::Root, crate::AppState, String, Arc<ScriptedBackend>) {
    let backend = Arc::new(ScriptedBackend::new());
    backend.script(
        &synra_plugin_host::Work::InvokeAction {
            name: "send".to_owned(),
            args_json: args_json.to_owned(),
        },
        answer,
    );
    let (root, state, token) =
        action_page_on(Backend::new(backend.clone()), b"\0asm\x01\0\0\0".to_vec());
    (root, state, token, backend)
}

fn action_page_on(
    backend: Backend,
    wasm: Vec<u8>,
) -> (crate::testing::Root, crate::AppState, String) {
    let (root, state) = crate::testing::file_state();
    let plugin = plugin_id();
    state
        .db()
        .insert_plugin(plugin.as_str(), true, true, "installed", crate::testing::FIXED_NOW_MS, crate::testing::FIXED_NOW_MS)
        .expect("a plugin row");
    state
        .db()
        .record_plugin_action(plugin.as_str(), "send", Some(r#"{"type":"object"}"#), Some("Send a message"))
        .expect("a declaration");

    let host = Arc::new(crate::plugin_host::DesktopHost::with_platform(
        Arc::clone(&state.db),
        Arc::clone(&state.platform),
    ));
    let mut set = synra_core::dispatch::PluginSet::new(backend, host);
    set.load(synra_core::dispatch::PluginSpec {
        id: plugin,
        version: "1.0.0".to_owned(),
        wasm,
        grants: CapabilitySet::new(),
        policy: synra_plugin_host::InstancePolicy::Stateless,
        limit: 4,
        quota: synra_plugin_host::quota::Quota::default(),
    })
    .expect("the plugin loads");

    let state = state.with_plugins(Arc::new(Mutex::new(set)));
    let token = hosted_page(&state);
    (root, state, token)
}

#[test]
fn the_actions_a_plugin_declared_are_listed_with_their_schema() {
    // ★ `args` is parsed rather than a JSON string inside JSON: the contract's `ActionDescriptor` declares it as
    // `unknown`, and a page rendering a form should not have to parse a string it was handed inside an object.
    let (_root, state, token, _backend) = action_page(synra_plugin_host::Scripted::Ok("null".to_owned()), "null");

    let answer = super::plugin_ui_rpc(&state, &token, "self.actions", &serde_json::json!({}))
        .expect("the list");

    assert_eq!(
        answer,
        serde_json::json!([{
            "name": "send",
            "args": {"type": "object"},
            "description": "Send a message",
        }])
    );
}

#[test]
fn an_action_runs_on_the_headless_half() {
    let (_root, state, token, backend) =
        action_page(synra_plugin_host::Scripted::Ok(r#"{"sent":true}"#.to_owned()), r#"{"to":"bob"}"#);

    let answer = super::plugin_ui_rpc(
        &state,
        &token,
        "self.invoke",
        &serde_json::json!({"action": "send", "args": {"to": "bob"}}),
    )
    .expect("the call");

    assert_eq!(answer, serde_json::json!({"sent": true}));
    assert_eq!(backend.calls(), 1, "the headless half was not run");
}

#[test]
fn an_action_this_plugin_never_declared_is_refused_before_the_call() {
    // ★ The ownership check the design asks for. `Work::InvokeAction` carries a name and nothing about who declared
    // it; without this lookup a page could name any action, which is one plugin driving another through a token it was
    // given for itself.
    let (_root, state, token, backend) = action_page(synra_plugin_host::Scripted::Ok("null".to_owned()), "null");

    let refused = super::plugin_ui_rpc(
        &state,
        &token,
        "self.invoke",
        &serde_json::json!({"action": "delete-everything"}),
    )
    .expect_err("that action is not this plugin's");

    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(refused.params.get("field").and_then(|value| value.as_str()), Some("action"));
    assert_eq!(backend.calls(), 0, "an undeclared action reached the headless half");
}

#[test]
fn a_structured_failure_keeps_the_code_the_plugin_chose() {
    let (_root, state, token, _backend) = action_page(
        synra_plugin_host::Scripted::Failed {
            code: "device_offline".to_owned(),
            message: "the other end is away".to_owned(),
            details: None,
        },
        "null",
    );

    let refused = super::plugin_ui_rpc(
        &state,
        &token,
        "self.invoke",
        &serde_json::json!({"action": "send"}),
    )
    .expect_err("the plugin refused");

    assert_eq!(refused.code, ErrorCode::DeviceOffline);
    assert_eq!(
        refused.params.get("reason").and_then(|value| value.as_str()),
        Some("the other end is away")
    );
}

#[test]
fn a_call_that_ran_out_of_time_is_a_plugin_timeout() {
    let (_root, state, token, _backend) = action_page(synra_plugin_host::Scripted::Timeout, "null");

    let refused = super::plugin_ui_rpc(
        &state,
        &token,
        "self.invoke",
        &serde_json::json!({"action": "send"}),
    )
    .expect_err("the call timed out");

    assert_eq!(refused.code, ErrorCode::PluginTimeout);
}

// ── Transfers ───────────────────────────────────────────────────────────────────────────────────

/// A page holding the transfer-send grant.
fn transfer_page() -> (crate::testing::Root, crate::AppState, String) {
    let (root, state) = crate::testing::file_state();
    let token = hosted_page_with(&state, &["file:transfer:send"]);
    (root, state, token)
}

#[test]
fn a_page_without_the_transfer_grant_cannot_send() {
    let state = crate::testing::state();
    let token = hosted_page(&state);

    let refused = super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.send",
        &serde_json::json!({"to": "11111111-1111-4111-8111-111111111111", "fileName": "a.txt", "data": ""}),
    )
    .expect_err("no grant");

    assert_eq!(refused.code, ErrorCode::CapabilityDenied);
    assert_eq!(
        refused.params.get("capability").and_then(|value| value.as_str()),
        Some("file:transfer:send")
    );
}

#[test]
fn a_transfer_argument_this_channel_cannot_honour_is_named() {
    use base64::Engine as _;

    let (_root, state, token) = transfer_page();
    let okay = base64::engine::general_purpose::STANDARD.encode(b"hello");

    // The grouping id was accepted by the signature and never reached the sender; forwarding it would be a promise.
    let refused = super::plugin_ui_rpc(
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
    let refused = super::plugin_ui_rpc(
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
    assert_eq!(refused.params.get("field").and_then(|value| value.as_str()), Some("fileName"));

    // The device id is a uuid; a made-up string is the caller's mistake, not a failed dial.
    let refused = super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.send",
        &serde_json::json!({"to": "not-a-uuid", "fileName": "a.txt", "data": okay}),
    )
    .expect_err("not a device id");
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert_eq!(refused.params.get("field").and_then(|value| value.as_str()), Some("to"));

    // And bytes that are not base64 never become a temp file.
    let refused = super::plugin_ui_rpc(
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
    assert_eq!(refused.params.get("field").and_then(|value| value.as_str()), Some("data"));
}

#[test]
fn a_page_uploads_a_file_in_chunks_and_the_host_keeps_it_until_finished() {
    use base64::Engine as _;

    let (_root, state, token) = transfer_page();
    let encoded = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);

    let begun = super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.begin",
        &serde_json::json!({
            "to": "11111111-1111-4111-8111-111111111111",
            "fileName": "big.bin",
            "totalBytes": 5,
        }),
    )
    .expect("the upload begins");
    let upload = begun
        .get("uploadId")
        .and_then(|value| value.as_str())
        .expect("an upload id")
        .to_owned();

    let uploads = state.paths.cache_dir().join("plugin-transfers");
    assert_eq!(
        std::fs::read_dir(&uploads).map(|entries| entries.count()).unwrap_or(0),
        1,
        "the upload owns one directory while it is open"
    );

    super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.chunk",
        &serde_json::json!({"uploadId": upload, "data": encoded(b"hello")}),
    )
    .expect("the chunk is appended");

    // More bytes than the page declared is refused rather than written, and the upload stays open so the page can still
    // abort it or retry with the right bytes.
    let refused = super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.chunk",
        &serde_json::json!({"uploadId": upload, "data": encoded(b"!")}),
    )
    .expect_err("over the declared size");
    assert_eq!(refused.code, ErrorCode::PayloadTooLarge);

    // An upload id this host does not know is answered as a bad argument, not as a send.
    let refused = super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.chunk",
        &serde_json::json!({"uploadId": "not-an-upload", "data": encoded(b"x")}),
    )
    .expect_err("no such upload");
    assert_eq!(refused.code, ErrorCode::InvalidArgument);

    let aborted = super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.abort",
        &serde_json::json!({"uploadId": upload}),
    )
    .expect("the upload aborts");
    assert_eq!(aborted.get("aborted"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(
        std::fs::read_dir(&uploads).map(|entries| entries.count()).unwrap_or(0),
        0,
        "the upload's directory went with it"
    );

}

#[test]
fn a_chunked_transfer_finished_with_the_managed_state_answers_before_the_dial() {
    // ★ The managed entry is the one the Tauri command uses. `finish` hands the upload to a worker and answers its id; the
    // dial (to a dead address) happens afterwards, so a failure arrives as a removed row rather than as a paused command.
    let (_root, state) = crate::testing::file_state();
    let token = hosted_page_with(&state, &["file:transfer:send"]);
    let device = synra_protocol::ids::DeviceId::new();
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: device.to_string(),
            display_name: "Study PC".to_owned(),
            platform: "windows".to_owned(),
            features: "[]".to_owned(),
            link_token: "unused-here".to_owned(),
            address: Some("127.0.0.1:1".to_owned()),
            paired_at_ms: 1_000,
        })
        .expect("a paired device");
    let state = std::sync::Arc::new(state);

    let begun = super::plugin_ui_rpc_arc(
        &state,
        &token,
        "transfer.begin",
        &serde_json::json!({"to": device.to_string(), "fileName": "a.bin", "totalBytes": 2}),
    )
    .expect("the upload begins");
    let upload = begun
        .get("uploadId")
        .and_then(|value| value.as_str())
        .expect("an upload id")
        .to_owned();
    super::plugin_ui_rpc_arc(
        &state,
        &token,
        "transfer.chunk",
        &serde_json::json!({"uploadId": upload, "data": "aGk="}),
    )
    .expect("the chunk is appended");

    let finished = super::plugin_ui_rpc_arc(
        &state,
        &token,
        "transfer.finish",
        &serde_json::json!({"uploadId": upload}),
    )
    .expect("finish answers before the dial");
    assert!(finished.get("transferId").is_some(), "an id, not a completed send");

    // The worker fails against port 1 and removes its row; the test waits for that rather than for a fixed sleep.
    //
    // ★ Ten seconds rather than two. The condition is right, but the budget assumed an idle machine: with the rest of the
    // suite running in parallel the worker can take longer than two seconds to be scheduled and to see the refused dial,
    // and the test then failed with "the failed send left no row" on a run where nothing was wrong. A condition-poll with
    // a generous ceiling is still a condition-poll; the assertion below is what decides, not the clock.
    for _ in 0..1_000 {
        if state.transfers.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(state.transfers.is_empty(), "the failed send left no row");
    assert!(
        !state.paths.cache_dir().join("plugin-transfers").read_dir().map(|entries| entries.count() > 0).unwrap_or(false),
        "the upload's files were left in the cache"
    );
}

#[test]
fn a_chunked_transfer_the_page_never_finishes_uses_no_send() {
    // ★ Beginning and aborting is the "page went away" path: the host must not dial anything, and the only thing left is
    // that the upload is gone.
    let (_root, state, token) = transfer_page();
    let begun = super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.begin",
        &serde_json::json!({
            "to": "11111111-1111-4111-8111-111111111111",
            "fileName": "a.bin",
            "totalBytes": 4,
        }),
    )
    .expect("the upload begins");
    let upload = begun.get("uploadId").and_then(|value| value.as_str()).expect("an id");

    let refused = super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.finish",
        &serde_json::json!({"uploadId": upload}),
    )
    .expect_err("the declared bytes never arrived");
    assert_eq!(refused.code, ErrorCode::InvalidArgument);
    assert!(state.transfers.is_empty(), "nothing was sent");
}

#[test]
fn a_file_larger_than_one_ipc_message_is_refused_by_size() {
    use base64::Engine as _;

    let (_root, state, token) = transfer_page();
    let too_big = vec![0u8; super::MAX_PLUGIN_TRANSFER_BYTES + 1];
    let data = base64::engine::general_purpose::STANDARD.encode(too_big);

    let refused = super::plugin_ui_rpc(
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
        refused.params.get("maxBytes").and_then(|value| value.as_u64()),
        Some(super::MAX_PLUGIN_TRANSFER_BYTES as u64)
    );
}

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

    let answer = super::plugin_ui_rpc(
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

    let refused = super::plugin_ui_rpc(
        &state,
        &token,
        "transfer.status",
        &serde_json::json!({"transferId": synra_protocol::ids::TransferId::new().to_string()}),
    )
    .expect_err("nothing by that id");

    assert_eq!(refused.code, ErrorCode::Unsupported);
}

#[test]
fn a_scoped_plugin_gets_a_window_label_without_a_slash() {
    // ★ The label is what makes "one page per plugin" findable: the id's slash cannot appear in a window label, and the
    // mapping has to be the same one `open_request` recorded or the second open would build a second window.
    let label = super::window_label_prefix(&plugin_id());
    assert_eq!(label, super::origin::label(&plugin_id()));
    assert!(
        label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    );
    assert_ne!(
        label,
        super::window_label_prefix(&synra_protocol::PluginId::parse("@other/hello").unwrap())
    );
}

#[test]
fn closing_a_page_ends_its_session() {
    // ★ The token is the page's authority. An entry that outlives its window keeps answering for a page nobody can see,
    // and a second window with the same label would inherit the first one's nonce.
    let state = crate::testing::state();
    let token = hosted_page(&state);
    let nonce: u128 = token.parse().expect("the token is the nonce in decimal");

    super::forget_window(&state, "page-rpc");

    assert!(state.hosted_ui.lock().get("page-rpc").is_none());
    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    assert!(state.ui_sessions.validate(nonce, &plugin_id(), "1.0.0", now).is_err(), "the session is gone");
}

#[cfg(feature = "tauri-shell")]
#[test]
fn a_plugin_page_is_served_with_a_policy_that_cannot_reach_the_network() {
    // ★ The design assigns the CSP to the protocol handler (08 §550). Without this header a page's network access depends
    // on a global setting the plugin cannot see. Only Tauri's local IPC transport may connect directly.
    let response = super::document_response(b"<html></html>".to_vec(), "text/html; charset=utf-8");

    assert_eq!(response.status(), 200);
    let policy = response
        .headers()
        .get("content-security-policy")
        .expect("the policy header")
        .to_str()
        .expect("a header string");

    assert!(policy.contains("default-src 'none'"), "{policy}");
    let connect = policy
        .split(';')
        .map(str::trim)
        .find_map(|directive| directive.strip_prefix("connect-src "))
        .expect("an explicit connection policy");
    assert_eq!(
        connect.split_whitespace().collect::<Vec<_>>(),
        ["ipc:", "https://ipc.localhost"],
        "no general network or plugin-origin connection allowance"
    );
    assert!(policy.contains("script-src 'self'"), "{policy}");
    assert!(policy.contains("form-action 'none'"), "{policy}");
}

#[test]
fn command_failures_keep_limits_and_cleanup_state_in_ui_errors() {
    struct RefusingCommand(PlatformError);
    impl CommandRunner for RefusingCommand {
        fn is_available(&self) -> bool {
            true
        }
        fn run(
            &self,
            _program: &str,
            _arguments: &[&str],
            _input: Option<&str>,
        ) -> Result<String, PlatformError> {
            Err(self.0.clone())
        }
    }
    for (error, code, kind) in [
        (
            PlatformError::RunnerBusy { limit: 8 },
            ErrorCode::RateLimited,
            "busy",
        ),
        (
            PlatformError::RunnerTimedOut {
                program: "fixture".to_owned(),
                timeout_ms: 20,
                cleanup_pending: true,
            },
            ErrorCode::Timeout,
            "timeout",
        ),
        (
            PlatformError::RunnerOutputTooLong {
                program: "fixture".to_owned(),
                stream: "stdout",
                max: 64,
            },
            ErrorCode::PayloadTooLarge,
            "outputLimit",
        ),
        (
            PlatformError::RunnerCleanupFailed {
                program: "fixture".to_owned(),
            },
            ErrorCode::Internal,
            "cleanupPending",
        ),
        (
            PlatformError::RunnerFailed {
                program: "fixture".to_owned(),
                reason: "sensitive-clipboard-content".to_owned(),
            },
            ErrorCode::Internal,
            "commandFailed",
        ),
    ] {
        let (_root, mut state, token, _) = platform_page(&["platform:read-clipboard"]);
        state.platform = Arc::new(CommandPlatform::with(
            Desktop::MacOs,
            Box::new(RefusingCommand(error)),
        ));
        let error = super::plugin_ui_rpc(&state, &token, "clipboard.read", &serde_json::json!({}))
            .expect_err("command refusal");
        assert_eq!(error.code, code);
        assert_eq!(error.params["kind"], kind);
        assert!(
            !error
                .params
                .to_string()
                .contains("sensitive-clipboard-content")
        );
        if kind == "timeout" {
            assert_eq!(error.params["timeoutMs"], 20);
            assert_eq!(error.params["cleanupPending"], true);
        }
    }
}
