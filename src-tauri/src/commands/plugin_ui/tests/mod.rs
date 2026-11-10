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

// ── The plugin's own actions ────────────────────────────────────────────────────────────────────

/// A state whose plugin is loaded, has declared one action, and whose backend answers `actions.invoke` with `answer`.
///
/// The script is keyed by the call's **payload**, so `args_json` must be what `self.invoke` will serialise: an action
/// asked for without arguments arrives as `null`, not as `{}`.
fn action_page(
    answer: synra_plugin_host::Scripted,
    args_json: &str,
) -> (
    crate::testing::Root,
    crate::AppState,
    String,
    Arc<ScriptedBackend>,
) {
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
        .insert_plugin(
            plugin.as_str(),
            true,
            true,
            "installed",
            crate::testing::FIXED_NOW_MS,
            crate::testing::FIXED_NOW_MS,
        )
        .expect("a plugin row");
    state
        .db()
        .record_plugin_action(
            plugin.as_str(),
            "send",
            Some(r#"{"type":"object"}"#),
            Some("Send a message"),
        )
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

// ── Transfers ───────────────────────────────────────────────────────────────────────────────────

/// A page holding the transfer-send grant.
fn transfer_page() -> (crate::testing::Root, crate::AppState, String) {
    let (root, state) = crate::testing::file_state();
    let token = hosted_page_with(&state, &["file:transfer:send"]);
    (root, state, token)
}

mod actions;
mod assets;
mod environment;
mod network;
mod opening;
mod platform;
mod rpc;
mod sessions;
mod storage;
mod transfer_admission;
mod transfer_status;
mod transfer_uploads;
