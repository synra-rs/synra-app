#![allow(clippy::needless_pass_by_value)]

use synra_ipc::{DevicesSnapshot, IdentitySnapshot, IpcError, PairingSnapshot, ScanOptions};

use crate::{commands, state::AppState};

#[tauri::command]
pub fn identity_get(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<IdentitySnapshot, IpcError> {
    commands::identity::identity_get(&state)
}

#[tauri::command]
pub fn identity_set_display_name(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    name: String,
) -> Result<(), IpcError> {
    commands::identity::identity_set_display_name(&state, &name)
}

#[tauri::command]
pub fn devices_snapshot(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<DevicesSnapshot, IpcError> {
    commands::devices::devices_snapshot(&state)
}

/// Starts a scan.
///
/// ★ `(async)`, and it is not a style choice: the body waits out the protocol's scan window (up to 1.2 seconds of
/// listening for answers), and a plain `#[tauri::command]` runs its body on the thread that received the invoke —
/// which is the window's. Tauri's `(async)` on a **synchronous** function puts it on the sync thread pool instead
/// (`sync_threadpool` in the macro's own vocabulary), so the window keeps painting while the network is asked. The
/// daemon makes the same move for the same reason, by running discovery on a thread of its own.
#[tauri::command(async)]
pub fn devices_scan_start(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    opts: ScanOptions,
) -> Result<String, IpcError> {
    commands::devices::devices_scan_start(&state, &opts)
}

#[tauri::command]
pub fn devices_scan_cancel(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    scan_id: String,
) -> Result<(), IpcError> {
    commands::devices::devices_scan_cancel(&state, &scan_id)
}

#[tauri::command(async)]
pub fn devices_forget(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    device_id: String,
) -> Result<(), IpcError> {
    commands::devices::devices_forget(&state, &device_id)
}
/// Starts pairing with a device: makes the code, dials, and answers with the code.
#[tauri::command(async)]
pub fn devices_pair(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    device_id: String,
) -> Result<PairingSnapshot, IpcError> {
    commands::pairing::devices_pair(&state, &device_id)
}

/// Answers the request on the desk: accept or refuse.
#[tauri::command(async)]
pub fn devices_pairing_respond(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    accepted: bool,
    block: Option<bool>,
) -> Result<PairingSnapshot, IpcError> {
    commands::pairing::devices_pairing_respond(&state, accepted, block.unwrap_or(false))
}

/// Withdraws this device's own request.
#[tauri::command(async)]
pub fn devices_pairing_cancel(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<PairingSnapshot, IpcError> {
    commands::pairing::devices_pairing_cancel(&state)
}

/// What the pairing panel should draw right now.
#[tauri::command(async)]
pub fn devices_pairing_snapshot(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<PairingSnapshot, IpcError> {
    commands::pairing::devices_pairing_snapshot(&state)
}

/// The host-only pointer consent and active control snapshot.
#[tauri::command]
pub fn devices_input_snapshot(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<synra_ipc::InputSnapshot, IpcError> {
    commands::input::devices_input_snapshot(&state)
}

/// Decide the current input request; plugin pages cannot reach host commands.
#[tauri::command]
pub fn devices_input_respond(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    session_id: String,
    allow: bool,
) -> Result<(), IpcError> {
    commands::input::devices_input_respond(&state, &session_id, allow)
}

/// Stop pointer control and release its buttons independently of the phone.
#[tauri::command]
pub fn devices_input_stop(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    session_id: String,
) -> Result<(), IpcError> {
    commands::input::devices_input_stop(&state, &session_id)
}

pub(super) fn dispatch(invoke: tauri::ipc::Invoke<tauri::Wry>) -> bool {
    let dispatch: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        identity_get,
        identity_set_display_name,
        devices_snapshot,
        devices_scan_start,
        devices_scan_cancel,
        devices_forget,
        devices_pair,
        devices_pairing_respond,
        devices_pairing_cancel,
        devices_pairing_snapshot,
        devices_input_snapshot,
        devices_input_respond,
        devices_input_stop,
    ];
    dispatch(invoke)
}
