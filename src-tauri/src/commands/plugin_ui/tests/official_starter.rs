//! Opt-in production install, activation and UI RPC gate for the independently built Starter package.

use parking_lot::Mutex;
use std::{path::PathBuf, sync::Arc};
use synra_plugin_host::{
    Backend,
    engine::{BackendKind, WasmtimeBackend},
};
use synra_protocol::{ErrorCode, PluginId};

#[test]
fn official_starter_installs_activates_and_answers_its_own_page() {
    std::thread::Builder::new()
        .name("starter-small-stack".to_owned())
        .stack_size(1024 * 1024)
        .spawn(exercise_starter)
        .expect("start a Windows-main-sized caller")
        .join()
        .expect("real Starter works on a 1 MiB caller stack");
}

fn exercise_starter() {
    let package = PathBuf::from(
        std::env::var_os("SYNRA_STARTER_PACKAGE")
            .expect("set SYNRA_STARTER_PACKAGE to the CLI-built Starter dist directory"),
    );
    let (_root, state) = crate::testing::file_state();
    let host = Arc::new(crate::plugin_host::DesktopHost::with_platform(
        Arc::clone(&state.db),
        Arc::clone(&state.platform),
    ));
    let engine = WasmtimeBackend::new(BackendKind::WasmtimeCranelift).expect("real Wasmtime");
    let state = state.with_plugins(Arc::new(Mutex::new(synra_core::dispatch::PluginSet::new(
        Backend::new(Arc::new(engine)),
        host,
    ))));
    const ID: &str = "@synra-plugin/starter";
    let installed =
        crate::commands::plugins::install_directory(&state, &package).expect("production install");
    assert_eq!(installed.plugin_id.as_str(), ID);
    crate::commands::plugins::plugins_consent_respond(
        &state,
        ID,
        &["action:declare:debug".to_owned(), "device:list".to_owned()],
    )
    .expect("explicit scoped grant");
    crate::commands::plugins::plugins_enable(&state, ID)
        .expect("activate the real guest and register debug");
    let request = super::opened(&state, ID);
    let parsed = url::Url::parse(&request.url).expect("actual parsed WebView URL");
    assert_eq!(parsed.as_str(), request.url);
    let ids = [PluginId::parse(ID).unwrap()];
    assert!(super::super::split_ui_url(parsed.as_str(), &ids).is_some());
    let (html, mime) = super::super::page_resource(&state, &request.label, parsed.as_str())
        .expect("actual extracted entry HTML");
    assert_eq!(mime, "text/html; charset=utf-8");
    let html = std::str::from_utf8(&html).expect("HTML UTF-8");
    assert!(html.contains("/assets/"));
    let script = html
        .split("src=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let script_url = parsed.join(script).expect("relative Vite asset");
    assert!(super::super::split_ui_url(script_url.as_str(), &ids).is_some());
    let (javascript, _) = super::super::page_resource(&state, &request.label, script_url.as_str())
        .expect("actual built JS");
    assert!(!javascript.is_empty());
    super::super::plugin_ui_ready(&state, &request.label).expect("page readiness");
    let token = state
        .hosted_ui
        .lock()
        .get(&request.label)
        .unwrap()
        .nonce
        .to_string();
    assert!(
        !request
            .label
            .contains(&format!("{:032x}", token.parse::<u128>().unwrap())),
        "labels never reveal the RPC nonce"
    );
    let actions =
        super::super::plugin_ui_rpc(&state, &token, "self.actions", &serde_json::json!({}))
            .unwrap();
    assert_eq!(
        super::super::plugin_ui_rpc(&state, &token, "device.list", &serde_json::json!({})).unwrap(),
        serde_json::json!([]),
        "the actual UI device-list grant reaches the production dispatcher"
    );
    assert_eq!(actions[0]["name"], "debug");
    let report =
        call(&state, &token, "debug", serde_json::json!({})).expect("real Rust diagnostic action");
    assert_eq!(report["pluginId"], ID);
    assert_eq!(report["hostSdkVersion"], synra_plugin_host::SDK_VERSION);
    assert_eq!(report["hostKind"], "desktop");
    assert_eq!(report["locale"], "en");
    assert_eq!(report["darkMode"], false);
    assert!(call(&state, &token, "undeclared", serde_json::json!({})).is_err());
    assert!(call(&state, &token, "debug", serde_json::Value::Null).is_err());
    assert_eq!(
        call(&state, "0", "debug", serde_json::json!({}))
            .unwrap_err()
            .code,
        ErrorCode::CapabilityDenied
    );
    exercise_lifecycle(&state, &package, &request, &token);
}

fn call(
    state: &crate::AppState,
    token: &str,
    action: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, synra_ipc::IpcError> {
    super::super::plugin_ui_rpc(
        state,
        token,
        "self.invoke",
        &serde_json::json!({"action":action,"args":args}),
    )
}

fn exercise_lifecycle(
    state: &crate::AppState,
    package: &std::path::Path,
    request: &super::OpenRequest,
    token: &str,
) {
    const ID: &str = "@synra-plugin/starter";
    crate::commands::plugins::plugins_disable(state, ID)
        .expect("disable through production command");
    assert_eq!(
        super::super::plugin_ui_rpc(state, token, "self.actions", &serde_json::json!({}))
            .expect_err("disabled pages lose all RPC authority")
            .code,
        ErrorCode::CapabilityDenied
    );
    crate::commands::plugins::plugins_enable(state, ID).expect("enable again");
    assert_eq!(
        call(state, token, "debug", serde_json::json!({}))
            .unwrap_err()
            .code,
        ErrorCode::CapabilityDenied,
        "enabling the same version must not resurrect an old page"
    );
    assert!(super::super::page_resource(state, &request.label, &request.url).is_err());
    assert!(super::super::plugin_ui_ready(state, &request.label).is_err());
    let replacement = super::opened(state, ID);
    assert_ne!(replacement.label, request.label);
    assert!(replacement.superseded.contains(&request.label));
    super::super::forget_window(state, &request.label);
    let second = state
        .hosted_ui
        .lock()
        .get(&replacement.label)
        .unwrap()
        .nonce
        .to_string();
    assert!(
        call(state, &second, "debug", serde_json::json!({})).is_ok(),
        "late destruction of the old document cannot revoke the new one"
    );
    assert!(super::super::page_resource(state, "foreign-window", &replacement.url).is_err());

    crate::commands::plugins::plugins_reload(state, ID).expect("explicit same-version reload");
    assert_eq!(
        call(state, &second, "debug", serde_json::json!({}))
            .unwrap_err()
            .code,
        ErrorCode::CapabilityDenied
    );
    let reloaded = super::opened(state, ID);
    let third = state
        .hosted_ui
        .lock()
        .get(&reloaded.label)
        .unwrap()
        .nonce
        .to_string();
    crate::commands::plugins::plugins_consent_respond(state, ID, &[])
        .expect("revoke action registration");
    assert_eq!(
        call(state, &third, "debug", serde_json::json!({}))
            .unwrap_err()
            .code,
        ErrorCode::CapabilityDenied
    );
    assert!(super::super::page_resource(state, &reloaded.label, &reloaded.url).is_err());
    assert!(super::super::stale_windows(state).contains(&reloaded.label));

    crate::commands::plugins::plugins_consent_respond(
        state,
        ID,
        &["action:declare:debug".to_owned()],
    )
    .unwrap();
    let restored = super::opened(state, ID);
    let fourth = state
        .hosted_ui
        .lock()
        .get(&restored.label)
        .unwrap()
        .nonce
        .to_string();
    assert_eq!(
        super::super::plugin_ui_rpc(state, &fourth, "device.list", &serde_json::json!({}))
            .expect_err("partial consent does not inherit the old device-list grant")
            .code,
        ErrorCode::CapabilityDenied
    );
    crate::commands::plugins::plugins_uninstall(state, ID, false).expect("uninstall");
    assert_eq!(
        call(state, &fourth, "debug", serde_json::json!({}))
            .unwrap_err()
            .code,
        ErrorCode::CapabilityDenied
    );
    crate::commands::plugins::install_directory(state, package)
        .expect("reinstall identical version");
    crate::commands::plugins::plugins_consent_respond(
        state,
        ID,
        &["action:declare:debug".to_owned()],
    )
    .unwrap();
    crate::commands::plugins::plugins_enable(state, ID).unwrap();
    assert_eq!(
        call(state, &fourth, "debug", serde_json::json!({}))
            .unwrap_err()
            .code,
        ErrorCode::CapabilityDenied
    );
    let final_page = super::opened(state, ID);
    let final_token = state
        .hosted_ui
        .lock()
        .get(&final_page.label)
        .unwrap()
        .nonce
        .to_string();
    assert!(call(state, &final_token, "debug", serde_json::json!({})).is_ok());
    super::super::forget_window(state, &final_page.label);
    assert_eq!(
        call(state, &final_token, "debug", serde_json::json!({}))
            .unwrap_err()
            .code,
        ErrorCode::CapabilityDenied
    );
}
