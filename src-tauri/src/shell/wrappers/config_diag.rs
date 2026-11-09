#![allow(clippy::needless_pass_by_value)]

use synra_ipc::{ConfigPatch, ConfigSnapshot, DiagExportOptions, DiagExportResult, IpcError};

use crate::{commands, state::AppState};

#[tauri::command]
pub fn config_get(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<ConfigSnapshot, IpcError> {
    commands::config::config_get(&state)
}

#[tauri::command]
pub fn config_set(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    patch: ConfigPatch,
) -> Result<(), IpcError> {
    commands::config::config_set(&state, &patch)
}

#[tauri::command]
pub fn diag_export(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    options: DiagExportOptions,
) -> Result<DiagExportResult, IpcError> {
    commands::diag::diag_export(&state, &options)
}

/// Runs the startup probes and answers them.
///
/// ★ No arguments: the questions are fixed and the answers are about this machine, not about what the caller wants
/// to know. A filter parameter would let a page hide a failing probe by not asking for it.
#[tauri::command]
pub fn diag_probes(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<synra_ipc::ProbeReportDto, IpcError> {
    commands::diag::diag_probes(&state)
}

/// The tail of the shell's own log.
///
/// `lines: Option<u32>` rather than an options struct, because there is one option. A struct for a single count
/// would be an IPC type that has to be mirrored in the TypeScript contract for no shape of its own.
#[tauri::command]
pub fn diag_logs_tail(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    lines: Option<u32>,
) -> Result<Vec<String>, IpcError> {
    commands::diag::diag_logs_tail(&state, lines)
}

/// The crash reports the next start would decide safe mode from, newest first.
#[tauri::command]
pub fn diag_crashes(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<Vec<synra_observe::crash::CrashReport>, IpcError> {
    commands::diag::diag_crashes(&state)
}

/// Deletes the crash reports that would make the next start enter safe mode.
#[tauri::command]
pub fn diag_crash_dismiss(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<(), IpcError> {
    commands::diag::diag_crash_dismiss(&state)
}

/// What the shell's metrics hold.
///
/// ★ No arguments, and that is the command's whole interface.
///
/// Metrics are read as a set rather than queried one at a time; a command that took a metric name would be a command
/// whose caller already knew what it was looking for, which is the opposite of what a diagnostic is for.
#[tauri::command]
pub fn diag_metrics(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<synra_observe::metrics::MetricsSnapshot, IpcError> {
    commands::diag::diag_metrics(&state)
}

pub(super) fn dispatch(invoke: tauri::ipc::Invoke<tauri::Wry>) -> bool {
    let dispatch: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        config_get,
        config_set,
        diag_export,
        diag_probes,
        diag_logs_tail,
        diag_crash_dismiss,
        diag_crashes,
        diag_metrics,
    ];
    dispatch(invoke)
}
