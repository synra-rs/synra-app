//! Zero-capability approval through the real command, store and loader.

use crate::{state::AppState, testing};

const ID: &str = "@test/no-permissions";

fn with_engine(state: AppState) -> AppState {
    let host = std::sync::Arc::new(crate::plugin_host::DesktopHost::with_platform(
        std::sync::Arc::clone(&state.db),
        std::sync::Arc::clone(&state.platform),
    ));
    let backend = synra_plugin_host::Backend::new(std::sync::Arc::new(
        synra_plugin_host::ScriptedBackend::new(),
    ));
    state.with_plugins(std::sync::Arc::new(parking_lot::Mutex::new(
        synra_core::dispatch::PluginSet::new(backend, host),
    )))
}

fn installed(enabled: bool) -> (testing::Root, AppState) {
    let (root, state) = testing::file_state();
    let directory = state.paths.plugin_dir(ID);
    std::fs::create_dir_all(&directory).expect("a private plugin directory");
    let manifest = serde_json::json!({
        "id": ID, "version": "1.0.0", "title": "No permissions",
        "publisher": { "id": "test" }, "sdk": { "min": "0.1.0" },
        "wit": synra_plugin_host::host_world(), "capabilities": [],
        "runtime": { "headless": { "artifact": "plugin.wasm", "stateful": false } }
    });
    std::fs::write(directory.join("manifest.json"), manifest.to_string()).expect("a manifest");
    std::fs::write(
        directory.join("plugin.wasm"),
        synra_testkit::component::inert_component().expect("a component"),
    )
    .expect("an artifact");
    state
        .db()
        .insert_plugin(
            ID,
            enabled,
            false,
            "installed",
            testing::FIXED_NOW_MS,
            testing::FIXED_NOW_MS,
        )
        .expect("an unapproved plugin");
    (root, with_engine(state))
}

#[test]
fn explicit_empty_consent_starts_a_zero_capability_plugin_and_survives_restart() {
    let (_root, state) = installed(true);
    let before = super::plugins_list(&state).expect("the list");
    assert_eq!(
        before.first().expect("the plugin").state,
        "awaiting_consent"
    );

    let approved = super::plugins_consent_respond(&state, ID, &[]).expect("explicit approval");
    assert_eq!(approved.state, "active");
    assert!(
        approved.requested.is_empty() && approved.granted.is_empty() && approved.denied.is_empty()
    );
    assert!(
        state.db().plugin_grants(ID).expect("grants").is_empty(),
        "no fake grant is written"
    );

    let restarted = with_engine(testing::reopen(&state));
    assert!(
        restarted
            .db()
            .plugins()
            .expect("rows")
            .first()
            .expect("the plugin")
            .trusted
    );
    super::plugins::plugins_reload(&restarted, ID)
        .expect("loads with no capability grants after restart");
    assert_eq!(
        super::plugins_list(&restarted)
            .expect("the list")
            .first()
            .expect("the plugin")
            .state,
        "active"
    );
}

#[test]
fn approving_a_disabled_zero_capability_plugin_does_not_enable_it() {
    let (_root, state) = installed(false);
    let approved = super::plugins_consent_respond(&state, ID, &[]).expect("explicit approval");
    assert_eq!(approved.state, "disabled");
    assert!(!approved.enabled);
    assert!(
        state
            .db()
            .plugins()
            .expect("rows")
            .first()
            .expect("the plugin")
            .trusted
    );
    super::plugins_enable(&state, ID).expect("the user enables it later");
    assert_eq!(
        super::plugins_list(&state)
            .expect("the list")
            .first()
            .expect("the plugin")
            .state,
        "active"
    );
}
