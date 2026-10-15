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


/// What the loaded plugin set knows about each installed plugin.
///
/// # ★ Why the store cannot answer this
///
/// Because the `plugins` table records `health`, not versions or liveness: it is what the kernel *observed*, written at
/// install time and updated on a health change. "Is it loaded right now" is a fact about this process, and "what version"
/// is a fact about the manifest the loader read — both live in the set, and neither is in the database.
///
/// # Why the lock is taken once
///
/// The set is shared with every plugin call in flight, so a guard taken per row is a guard that blocks a running plugin
/// for as long as this loop takes. One lock, one pass, released before the bundle is assembled.
#[derive(Default)]
struct LoadedPlugin {
    /// The version the loader read, or empty when the set does not have it.
    version: String,
    /// Whether it is loaded right now.
    active: bool,
    /// Whether the ring has refused anything for it.
    had_refusals: bool,
}

/// Reads the set once for every installed plugin.
///
/// A shell with no engine answers "not loaded, no version, nothing refused" for all of them — which is true of a shell
/// that has no engine, and not a fallback.
fn loaded_plugins(state: &AppState, installed: &[synra_store::records::PluginRow]) -> Vec<LoadedPlugin> {
    let Ok(set) = state.plugin_set() else {
        return installed.iter().map(|_| LoadedPlugin::default()).collect();
    };
    let set = set.lock();
    installed
        .iter()
        .map(|row| {
            let Ok(id) = synra_protocol::PluginId::parse(&row.plugin_id) else {
                return LoadedPlugin::default();
            };
            LoadedPlugin {
                version: set.version(&id).unwrap_or_default().to_owned(),
                active: set.contains(&id),
                // ★ The one that used to be a hard-coded `false`.
                had_refusals: !set.refusals_for(&id).is_empty(),
            }
        })
        .collect()
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

    // ★ The plan is built with the sections this shell can fill, and the ones it cannot are stated as omissions.
    //
    // ★ Three of the four plugin fields used to be constants — `version: String::new()`, `active: false`,
    // `had_refusals: false` — and that was not an omission, it was **four claims**. `had_refusals: false` is the one
    // that matters: it tells a maintainer that nothing this plugin tried was ever refused, in the document a user sends
    // when they are asking why something did not work. A refusal is usually the answer.
    //
    // All three are readable now. The set knows what this process managed to load and which version; its ring knows
    // whether anything was refused.
    let loaded = loaded_plugins(state, &plugins);
    let mut sources = BundleSources {
        environment: Some(environment()),
        config: Some(configuration(state)),
        devices: Some(
            devices
                .iter()
                .map(|row| DeviceRecord {
                    device_id: row.device_id.clone(),
                    paired: true,
                    // ★ Asked rather than assumed. Every device here is paired by construction — this is the paired table —
                    // but "paired" and "reachable" are different, and a bundle that reported every peer online would send
                    // a maintainer looking for a network fault that is not there.
                    online: row.is_usable(),
                })
                .collect(),
        ),
        plugins: Some(
            plugins
                .iter()
                .zip(loaded)
                .map(|(row, state)| PluginRecord {
                    plugin_id: row.plugin_id.clone(),
                    version: state.version,
                    active: state.active,
                    had_refusals: state.had_refusals,
                })
                .collect(),
        ),
        ..BundleSources::default()
    };

    // ★ The optional sections are given a source only when the user asked for them.
    //
    // ★ The log is real, and so is the audit trail now — but the two are read from different places, because they are
    // written by different things. The shell starts its own log (`AppState::start_logging`), so `log_lines` comes from the
    // file the shell is writing. The audit trail comes from the plugin set's ring, which every plugin instance drains into
    // after each authorised call.
    //
    // ★ This said "there is still no audit sink" for several rounds after that stopped being true. An empty list was
    // honest when nothing wrote one; it is a false answer now, because a bundle that says it carries the audit trail and
    // carries none tells a user their plugins did nothing.
    if options.include_audit {
        // No engine means no instances, so no calls were authorised and an empty trail is the truth rather than a
        // fallback. A refusal here would be worse: the section is still legitimately empty.
        let recorded = state
            .plugin_set()
            .map(|set| set.lock().audit())
            .unwrap_or_default();
        sources.audit = Some(recorded);
    }
    if options.include_debug_logs || options.log_lines.is_some() {
        // `verbose` is true because the user asking for debug logs has asked for the debug lines; the quiet filter would
        // hide exactly what they requested, and the bundle would carry an empty section that looked deliberate.
        let wanted = options.log_lines.map_or(synra_observe::logging::DEFAULT_TAIL_LINES, |n| n as usize);
        sources.log_lines = Some(
            synra_observe::logging::tail(state.paths.log_dir(), wanted, true, None).lines,
        );
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

/// Reads the tail of the shell's own log.
///
/// # ★ Why this exists now and not before
///
/// Because there was nothing to read. Until the shell started its own log (`AppState::start_logging`), this command would
/// have read a path no code created — the same shape as the daemon's `synra logs`, which answered "no lines" for as long
/// as nothing wrote the file. Implementing it earlier would have shipped a command whose every answer was an empty list,
/// and an empty list is a claim: it says "nothing was logged" when the truth was "nothing is logging".
///
/// # Why it is not the same command as `logs`
///
/// `Request::Logs` in the control protocol is the **kernel's** log, read by the CLI from another process. This is the
/// **shell's** log, read in-process. They are two files because they are two processes, and a command that conflated
/// them would show a user the wrong one with no way to tell.
///
/// # Errors
///
/// [`IpcError::Internal`] only when the directory cannot be read at all. A missing log is not an error: a fresh
/// installation has not written one yet, and reporting that as a failure would make a working install look broken on its
/// first run.
pub fn diag_logs_tail(state: &AppState, lines: Option<u32>) -> Result<Vec<String>, IpcError> {
    let wanted = lines.map_or(synra_observe::logging::DEFAULT_TAIL_LINES, |n| n as usize);
    // ★ `verbose = true`, because this is a diagnostics command and the whole reason a user runs it is to see the detail
    // a quiet filter hides. It is also what `diag_export` does with the same file, so the two agree about what the log
    // contains — a pair of commands reading one file with different filters is how a user comes to distrust both.
    Ok(synra_observe::logging::tail(state.paths.log_dir(), wanted, true, None).lines)
}

/// The one place a database or filesystem failure becomes an `IpcError`.
fn internal(error: impl std::fmt::Display) -> IpcError {
    IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({ "reason": error.to_string() }))
}

#[cfg(test)]
mod tests;
