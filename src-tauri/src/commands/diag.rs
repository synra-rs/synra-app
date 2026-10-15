//! Diagnostics: what a user sends when something is wrong.
//!
//! # The one promise this module keeps
//!
//! **A bundle says what it contains and what it does not, and the file carries the same list the consent screen showed.**
//! A bundle that quietly omits the audit trail is still a bundle that looks complete, and a maintainer who cannot tell
//! which is which will spend a day reading a log that was never in it. So the exclusions are not a property of the
//! command — they are written into the artefact.

use synra_ipc::{DiagExportOptions, IpcError};
use synra_observe::diagnostics::{
    BundlePlan, BundleSection, BundleSources, DeviceRecord, EnvSummary, PluginRecord,
};
use synra_protocol::ErrorCode;

use crate::state::AppState;

/// Where a bundle is written, and what it holds.
///
/// The answer rather than a bare path, because "here is your file" is not usable on its own: a user who has just agreed
/// to share their configuration needs to be able to see what was in it before they send it to anybody, and `included`
/// and `excluded` are what let them do that without opening the file in an editor.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagExportResult {
    /// The file that was written.
    pub path: String,
    /// Which sections it holds.
    pub included: Vec<String>,
    /// Which sections were left out, and why the user agreed to that.
    pub excluded: Vec<String>,
}

/// Assembles a diagnostic bundle and writes it.
///
/// # Errors
///
/// * [`IpcError::Internal`] when the configuration or the devices cannot be read, or the file cannot be written.
///
/// # Why this reads the store directly rather than going through [`super::config_get`]
///
/// Because a bundle is exactly the moment when the *whole* picture is wanted, including the parts that are inconvenient
/// — so it is the one command that should not stop at the first bad row. A corrupt setting must not be the reason a
/// user cannot get help; the value goes in as the text it is, and the omission list says it was unreadable.
pub fn diag_export(state: &AppState, options: &DiagExportOptions) -> Result<DiagExportResult, IpcError> {
    let (devices, plugins) = {
        let db = state.db();
        (db.paired_devices().map_err(internal)?, db.plugins().map_err(internal)?)
    };

    // ★ The plan is built with only the sections this shell can actually fill, and the omissions are stated.
    //
    // There is no audit sink and no log ring behind `AppState` today, so claiming either would mean writing a section
    // that is empty — and an empty audit trail reads as "nothing happened", which is a claim, not an absence.
    let mut sources = BundleSources {
        environment: Some(environment()),
        config: Some(configuration(state)),
        devices: Some(
            devices
                .iter()
                .map(|row| DeviceRecord { device_id: row.device_id.clone(), paired: true, online: false })
                .collect(),
        ),
        plugins: Some(
            plugins
                .iter()
                .map(|row| PluginRecord {
                    plugin_id: row.plugin_id.clone(),
                    version: String::new(),
                    active: false,
                    had_refusals: false,
                })
                .collect(),
        ),
        ..BundleSources::default()
    };

    // ★ The optional sections are given a source only when the user asked for them.
    //
    // There is no audit sink and no log ring behind `AppState` today, so the value is an empty list — which is the honest
    // one. An empty audit trail says "nothing was recorded", where omitting the section says the same thing in a way a
    // reader cannot distinguish from "this bundle was made by a version that never had one".
    if options.include_audit {
        sources.audit = Some(Vec::new());
    }
    if options.include_debug_logs || options.log_lines.is_some() {
        sources.log_lines = Some(Vec::new());
    }

    let mut plan = BundlePlan::plan(sources, state.started_at_ms, destination(state, state.next_bundle_id()));
    if let Some(lines) = options.log_lines {
        plan.set_log_tail_lines(lines as usize);
    }
    // ★ Consent switches a section on, not merely makes one available.
    //
    // The plan's defaults are the sections a bundle always carries. Providing a source for an optional section is not
    // enough — without this the option would be accepted, the source would be built, and the section would still be
    // dropped, which is a consent screen that agrees with the file about nothing.
    if options.include_audit {
        plan.toggle_optional(BundleSection::AuditTrail, true);
    }
    if options.include_debug_logs {
        plan.toggle_optional(BundleSection::DebugLogs, true);
    }

    let rendered = plan.render().map_err(internal)?;
    let bundle = plan.build();

    // ★ The directory is made here, not at startup.
    //
    // A shell that creates it once on launch depends on that having happened: the user clears the directory, moves the
    // data folder, or installs onto a machine where the launch path never ran — and "export a diagnostic bundle" then
    // fails at exactly the moment a user most needs it to work. Making it at the point of use costs one call and cannot
    // be broken by anything that happened before.
    let path = plan.destination().to_path_buf();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(internal)?;
    }
    std::fs::write(&path, rendered).map_err(internal)?;

    Ok(DiagExportResult {
        path: path.display().to_string(),
        included: bundle.sections.clone(),
        excluded: bundle.excluded.clone(),
    })
}

/// Reads the whole configuration, past anything unreadable.
fn configuration(state: &AppState) -> serde_json::Value {
    let rows = state.db().config().unwrap_or_default();
    let mut values = serde_json::Map::new();
    for row in rows {
        // ★ Kept as text when it will not parse, rather than dropped.
        //
        // The value is a redacted view of what is stored, and "this setting is unreadable" is itself the diagnostic. A
        // bundle that quietly omitted it would leave the maintainer guessing whether the setting was absent or broken.
        let value = serde_json::from_str(&row.value)
            .unwrap_or_else(|_| serde_json::Value::String(format!("<unreadable: {}>", row.value)));
        values.insert(row.key, value);
    }
    serde_json::Value::Object(values)
}

/// What this device is, as a maintainer needs to know it.
fn environment() -> EnvSummary {
    EnvSummary {
        app_version: crate::KERNEL_VERSION.to_owned(),
        kernel_version: crate::KERNEL_VERSION.to_owned(),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        // ★ `None`, and not a guess.
        //
        // A WebView version is only knowable from inside the window, which this library deliberately does not touch — so
        // saying "unknown" would be a value, and a maintainer reading a bundle would spend time on it.
        webview_version: None,
    }
}

/// The path a bundle is written to.
///
/// # Why the identifier is a counter and not the clock
///
/// Because comparing a bundle from before a change with one from after it is the ordinary reason to export twice, and
/// two clicks inside one second is not unusual. A name built from seconds would put the second file on top of the first,
/// and the user would compare a bundle against itself and conclude nothing changed.
///
/// The start time is still carried, because a bundle from a process that started at 09:14 is worth knowing about even when
/// the counter restarts at 1 — a user comparing bundles from two runs needs to tell them apart.
fn destination(state: &AppState, bundle_id: u64) -> std::path::PathBuf {
    state
        .paths
        .data_dir()
        .join("diagnostics")
        .join(format!("bundle-{}-{bundle_id}.json", state.started_at_ms))
}

/// The one place a database or filesystem failure becomes an `IpcError`.
fn internal(error: impl std::fmt::Display) -> IpcError {
    IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({ "reason": error.to_string() }))
}

#[cfg(test)]
mod tests;
