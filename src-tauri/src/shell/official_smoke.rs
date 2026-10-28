//! Explicit, isolated desktop acceptance of the independently built Starter package.

use std::{ffi::OsStr, path::PathBuf, sync::Arc};
use tauri::Manager as _;

const IDENTIFIER: &str = "com.synra.ui-smoke";
const PLUGIN: &str = "@synra-plugin/starter";
const GRANTS: [&str; 2] = ["action:declare:debug", "device:list"];

pub(super) fn package(app: &tauri::AppHandle) -> Result<Option<PathBuf>, String> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new("--starter-ui-smoke")) {
        return Ok(None);
    }
    let package = args
        .next()
        .map(PathBuf::from)
        .ok_or("missing Starter package")?;
    if args.next().is_some() {
        return Err("unexpected smoke arguments".to_owned());
    }
    let schemes = app
        .config()
        .plugins
        .0
        .get("deep-link")
        .and_then(|config| config.pointer("/desktop/schemes"))
        .and_then(serde_json::Value::as_array);
    if app.config().identifier != IDENTIFIER || !schemes.is_some_and(Vec::is_empty) {
        return Err(
            "smoke requires its isolated identifier and empty deep-link schemes".to_owned(),
        );
    }
    Ok(Some(package))
}

pub(super) fn run(app: &tauri::AppHandle, package: &std::path::Path) -> Result<(), String> {
    let state = app.state::<Arc<crate::AppState>>();
    prepare(&state, package)?;
    super::wrappers::plugin_ui_open(app.clone(), state, PLUGIN.to_owned())
        .map_err(|error| format!("smoke window: {error:?}"))
}

fn prepare(state: &crate::AppState, package: &std::path::Path) -> Result<(), String> {
    let installed = state.db().plugins().map_err(|error| error.to_string())?;
    let report = match installed.as_slice() {
        [] => crate::commands::plugins::install_directory(state, package),
        [fixture] if fixture.plugin_id == PLUGIN => {
            crate::commands::plugins::update_directory(state, PLUGIN, package)
        }
        _ => return Err("smoke refuses profiles containing other plugins".to_owned()),
    }
    .map_err(|error| format!("smoke install: {error:?}"))?;
    let manifest = crate::commands::plugins::read_manifest(state, report.plugin_id.as_str())
        .ok_or("smoke manifest unavailable")?;
    if report.plugin_id.as_str() != PLUGIN
        || manifest.capabilities.len() != GRANTS.len()
        || !GRANTS
            .iter()
            .all(|grant| manifest.capabilities.iter().any(|cap| cap.cap == *grant))
    {
        return Err("smoke accepts only the local debug Starter fixture".to_owned());
    }
    let grants: Vec<String> = GRANTS.iter().map(|grant| (*grant).to_owned()).collect();
    crate::commands::plugins::plugins_consent_respond(state, PLUGIN, &grants)
        .map_err(|error| format!("smoke fixture consent: {error:?}"))?;
    crate::commands::plugins::plugins_enable(state, PLUGIN)
        .map_err(|error| format!("smoke activation: {error:?}"))?;
    Ok(())
}
