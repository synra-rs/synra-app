//! The window.
//!
//! # Why almost nothing is here
//!
//! Because every command's body lives in the library. This file has one job: build the state, register the handlers,
//! open a window. The split is not tidiness — it is what lets `cargo test --no-default-features` run all of the
//! command logic with no WebView2, no window and no `tauri` compiled in at all. A test that needs a window is a test
//! that does not run on a machine without one, and a test that does not run is not a test.
//!
//! So the window is here, the logic is not, and the amount of code in this file is the amount of code that can only
//! be checked by looking at a screen.

use std::sync::Arc;

use parking_lot::Mutex;
use synra_app::{commands, commands::diag::DiagExportResult, state::AppState};
use synra_store::{Paths, SynraDb};
use tauri::Manager;

// ─────────────────────────────────────────────────────────────────────────────
// The twelve wrappers
// ─────────────────────────────────────────────────────────────────────────────
//
// ★ Why each command is written twice.
//
// `#[tauri::command]` generates a module (`__cmd__identity_get`) and a symbol per command, and it does that by
// referring to `tauri::State`. The library deliberately does not depend on `tauri` — `cargo test --no-default-features`
// runs the bodies with no WebView in the build graph at all — so the bodies in `commands::*` are plain functions and
// the attribute has to be applied out here, in the one file that is only built when the shell is.
//
// The wrappers are as thin as the signature allows: take the arguments, call, return what came back. None of them
// decides anything, because a decision made here is a decision no test can reach.
//
// ★ `needless_pass_by_value` is switched off for this block, and it is the one place in the crate where that is right.
//
// Every argument here is owned because `#[tauri::command]` deserialises it that way — the macro writes the
// `Deserialize` bound into the generated `__cmd__*` module, so `&str` is not an option without changing what the
// command accepts over the wire. The lint is correct everywhere else in this repository and wrong in exactly this
// block, which is the distinction an `allow` is for.

#[allow(clippy::needless_pass_by_value)]
mod wrappers {
    use synra_ipc::{
        ConfigPatch, ConfigSnapshot, DevicesSnapshot, DiagExportOptions, IdentitySnapshot, IpcError, PluginSnapshot,
        ScanOptions,
    };

    use super::{AppState, commands, DiagExportResult};

    #[tauri::command]
    pub fn identity_get(state: tauri::State<'_, AppState>) -> Result<IdentitySnapshot, IpcError> {
        commands::identity::identity_get(&state)
    }

    #[tauri::command]
    pub fn identity_set_display_name(
        state: tauri::State<'_, AppState>,
        name: String,
    ) -> Result<(), IpcError> {
        commands::identity::identity_set_display_name(&state, &name)
    }

    #[tauri::command]
    pub fn devices_snapshot(state: tauri::State<'_, AppState>) -> Result<DevicesSnapshot, IpcError> {
        commands::devices::devices_snapshot(&state)
    }

    #[tauri::command]
    pub fn devices_scan_start(state: tauri::State<'_, AppState>, opts: ScanOptions) -> Result<String, IpcError> {
        commands::devices::devices_scan_start(&state, &opts)
    }

    #[tauri::command]
    pub fn devices_scan_cancel(state: tauri::State<'_, AppState>, scan_id: String) -> Result<(), IpcError> {
        commands::devices::devices_scan_cancel(&state, &scan_id)
    }

    #[tauri::command]
    pub fn devices_forget(state: tauri::State<'_, AppState>, device_id: String) -> Result<(), IpcError> {
        commands::devices::devices_forget(&state, &device_id)
    }

    #[tauri::command]
    pub fn plugins_list(state: tauri::State<'_, AppState>) -> Result<Vec<PluginSnapshot>, IpcError> {
        commands::plugins::plugins_list(&state)
    }

    #[tauri::command]
    pub fn plugins_enable(state: tauri::State<'_, AppState>, plugin_id: String) -> Result<(), IpcError> {
        commands::plugins::plugins_enable(&state, &plugin_id)
    }

    #[tauri::command]
    pub fn plugins_disable(state: tauri::State<'_, AppState>, plugin_id: String) -> Result<(), IpcError> {
        commands::plugins::plugins_disable(&state, &plugin_id)
    }

    /// Records the user's decision about what a plugin may do.
    #[tauri::command]
    pub fn plugins_consent_respond(
        state: tauri::State<'_, AppState>,
        plugin_id: String,
        granted: Vec<String>,
    ) -> Result<PluginSnapshot, IpcError> {
        commands::plugins::plugins_consent_respond(&state, &plugin_id, &granted)
    }

    #[tauri::command]
    pub fn config_get(state: tauri::State<'_, AppState>) -> Result<ConfigSnapshot, IpcError> {
        commands::config::config_get(&state)
    }

    #[tauri::command]
    pub fn config_set(state: tauri::State<'_, AppState>, patch: ConfigPatch) -> Result<(), IpcError> {
        commands::config::config_set(&state, &patch)
    }

    #[tauri::command]
    pub fn diag_export(
        state: tauri::State<'_, AppState>,
        options: DiagExportOptions,
    ) -> Result<DiagExportResult, IpcError> {
        commands::diag::diag_export(&state, &options)
    }

    /// The tail of the shell's own log.
    ///
    /// `lines: Option<u32>` rather than an options struct, because there is one option. A struct for a single count
    /// would be an IPC type that has to be mirrored in the TypeScript contract for no shape of its own.
    #[tauri::command]
    pub fn diag_logs_tail(
        state: tauri::State<'_, AppState>,
        lines: Option<u32>,
    ) -> Result<Vec<String>, IpcError> {
        commands::diag::diag_logs_tail(&state, lines)
    }
}

use wrappers::{
    config_get, config_set, devices_forget, devices_scan_cancel, devices_scan_start, devices_snapshot, diag_export,
    diag_logs_tail, identity_get, identity_set_display_name, plugins_consent_respond, plugins_disable, plugins_enable,
    plugins_list,
};

fn main() -> std::process::ExitCode {
    // ★ The refusal is printed, not panicked.
    //
    // A shell that cannot open its database has nothing to show, and a panic in a windowed app on Windows is a dialog
    // that says "synra-app.exe has stopped working" — which tells a user nothing and tells whoever triages it less.
    // The message here names the actual cause.
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(reason) => {
            eprintln!("synra: {reason}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Builds and runs the window.
///
/// # Errors
///
/// A message saying what stopped it.
fn run() -> Result<(), String> {
    tauri::Builder::default()
        .setup(|app| {
            let state = app_state(app.handle())?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            identity_get,
            identity_set_display_name,
            devices_snapshot,
            devices_scan_start,
            devices_scan_cancel,
            devices_forget,
            plugins_list,
            plugins_enable,
            plugins_disable,
            plugins_consent_respond,
            config_get,
            config_set,
            diag_export,
            diag_logs_tail,
        ])
        .run(tauri::generate_context!())
        .map_err(|error| format!("the window could not be opened: {error}"))
}

/// The state, over the real database in the real place.
///
/// # Errors
///
/// A message saying what stopped it. The three ways this can fail are a directory that cannot be created, a database
/// that cannot be opened or migrated, and an identity that cannot be written — and each one is a fact about the user's
/// machine rather than a bug in the code, so each is reported in words rather than as an opaque error code.
fn app_state(app: &tauri::AppHandle) -> Result<AppState, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("the data directory could not be found: {error}"))?;
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| format!("the cache directory could not be found: {error}"))?;

    let paths = Paths::new(data_dir, cache_dir);
    paths
        .ensure_directories()
        .map_err(|error| format!("the data directories could not be created: {error}"))?;

    let mut db = SynraDb::open(&paths.db_path())
        .map_err(|error| format!("the database could not be opened: {error}"))?;
    // Migrated on the way in, so a shell that starts on an older database does not fail every command with a message
    // about a missing table.
    db.migrate().map_err(|error| format!("the database could not be migrated: {error}"))?;

    // ★ The platform string the kernel stores, taken from the build target rather than from `std::env::consts`.
    //
    // They usually agree, and they disagree in the cross-compiled case — and the identity records the platform at first
    // launch, so a wrong answer here is written to disk and then believed by every peer for the life of the install.
    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    };

    let state = AppState::new(db, paths, platform, now_ms())
        .map_err(|error| format!("the identity could not be prepared: {error:?}"))?;

    let mut state = with_plugins_or_refuse(state);
    // ★ The shell's own log, started before anything else can go wrong — and its failure is reported rather than fatal.
    // A window that refuses to open because it cannot write a diagnostic file trades the user's actual work for its
    // record, and `diag_export` says the section is absent instead of claiming there was nothing to log.
    if let Err(error) = state.start_logging() {
        eprintln!("synra-app: logging is not running: {error}");
    }

    Ok(state)
}

/// The plugin set when an engine starts, and a shell without one when it does not.
///
/// # Errors
///
/// None. This is the one failure in start-up that does not stop the shell, and that is the whole point of the function.
///
/// ★ An engine that will not start — a missing system library, a CPU feature this build was not compiled for — is a shell
/// that still shows devices, settings and diagnostics. Dropping the window over it would turn a recoverable condition
/// into an unusable application, and the plugin half is the smaller loss.
fn with_plugins_or_refuse(state: AppState) -> AppState {
    let kind = synra_plugin_host::BackendKind::default_for(synra_plugin_host::HostKind::Desktop);
    match synra_plugin_host::engine::WasmtimeBackend::new(kind) {
        Ok(engine) => {
            let set = synra_core::dispatch::PluginSet::new(synra_plugin_host::Backend::new(Arc::new(engine)));
            state.with_plugins(Arc::new(Mutex::new(set)))
        }
        Err(error) => {
            // ★ Said once, here, and then carried by every command that needs a plugin.
            //
            // `AppState::plugin_set` is where the user-facing refusal is produced, with the reason attached. Saying it
            // again per command would be two statements of one fact, and the second is the one nobody reads.
            eprintln!("synra: plugins will not run in this build: {error}");
            state
        }
    }
}

/// The clock, in milliseconds since the epoch.
///
/// A clock before 1970 reads as zero rather than refusing: refusing to open a window over it would help nobody, and the
/// value only has to be monotonic within this process to be useful in a log line.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX))
}
