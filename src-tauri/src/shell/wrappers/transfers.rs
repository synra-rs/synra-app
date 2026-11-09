#![allow(clippy::needless_pass_by_value)]

use synra_ipc::IpcError;

use crate::{commands, state::AppState};

/// Abandons a transfer, and refuses one that is not in flight.
#[tauri::command]
pub fn transfer_cancel(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    transfer_id: String,
) -> Result<(), IpcError> {
    commands::transfers::transfer_cancel(&state, &transfer_id)
}

/// Answers the prompt for an incoming transfer.
///
/// The listener thread is waiting on this answer, and the file is accepted or refused when it arrives — so this is a
/// decision about a peer's connection, not a preference stored for later.
#[tauri::command(async)]
pub fn transfer_respond(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    transfer_id: String,
    accept: bool,
    remember: Option<bool>,
) -> Result<(), IpcError> {
    commands::transfers::transfer_respond(&state, &transfer_id, accept, remember)
}

/// Sends a file to a paired device, and answers the transfer it started.
///
/// ★ **It answers now, not when the file has moved.** The dial and the block loop run on a worker thread that owns
/// the transfer's row and its cancel token; what this returns is the identifier the panel needs in order to show the
/// transfer, and to stop it. The refusals that can happen before a worker exists — an unknown device, a file that
/// cannot be planned — are still errors here, because a caller that has a file but no thread must be told.
#[tauri::command]
pub fn transfer_send_file(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    request: synra_ipc::dto::SendFileRequest,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    commands::transfers::transfer_send_file_start(state.inner(), &request)
}

/// Sends text as a file, and answers the transfer it started.
///
/// As [`transfer_send_file`](Self::transfer_send_file): the temporary file is written before the worker starts, and the
/// worker removes it when it is done, whichever way it ends.
#[tauri::command]
pub fn transfer_send_text(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    request: synra_ipc::dto::SendTextRequest,
) -> Result<synra_protocol::ids::TransferId, IpcError> {
    commands::transfers::transfer_send_text_start(state.inner(), &request)
}

/// The transfers in flight.
#[tauri::command]
pub fn transfer_list(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Vec<synra_ipc::dto::TransferSnapshot> {
    commands::transfers::transfer_list(&state)
}

/// The files that have landed in the inbox, newest first.
#[tauri::command]
pub fn transfer_received(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Vec<synra_ipc::dto::ReceivedFile> {
    commands::transfers::transfer_received(&state)
}

/// Opens a file that landed in the inbox, with the platform's default application.
#[tauri::command]
pub fn transfer_open(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    path: String,
) -> Result<(), IpcError> {
    commands::transfers::transfer_open(&state, &path)
}

/// Shows a file that landed in the inbox in the platform's file manager.
#[tauri::command]
pub fn transfer_reveal(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    path: String,
) -> Result<(), IpcError> {
    commands::transfers::transfer_reveal(&state, &path)
}

/// Deletes a file that landed in the inbox.
#[tauri::command]
pub fn transfer_discard(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    path: String,
) -> Result<(), IpcError> {
    commands::transfers::transfer_discard(&state, &path)
}

pub(super) fn dispatch(invoke: tauri::ipc::Invoke<tauri::Wry>) -> bool {
    let dispatch: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        transfer_cancel,
        transfer_respond,
        transfer_send_file,
        transfer_send_text,
        transfer_list,
        transfer_received,
        transfer_open,
        transfer_reveal,
        transfer_discard,
    ];
    dispatch(invoke)
}
