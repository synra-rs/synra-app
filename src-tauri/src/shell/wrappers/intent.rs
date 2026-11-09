#![allow(clippy::needless_pass_by_value)]

use synra_ipc::{IntentDecisionDto, IntentSnapshot, IntentSourceDto, IpcError};
use synra_protocol::ids::IntentId;

use crate::{commands, state::AppState};

// ★ The four that had no implementation until the orchestration moved out of the daemon binary.
//
// The reason recorded against them used to be "needs the intent board behind AppState", which was false: the board
// was already in the state. What was missing was roughly 250 lines of routing that only a binary could reach.
#[tauri::command]
pub fn intent_dispatch(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    source: IntentSourceDto,
) -> Result<IntentId, IpcError> {
    commands::intent::intent_dispatch(&state, &source)
}

/// Every intent the board is holding, for the home page's "received" panel.
///
/// No arguments, and that is the command's whole interface: it answers about the board rather than about one intent —
/// which is the distinction `intent_snapshot` **below** it draws, and the one the interface missed while it called
/// that command and read a list out of it.
#[tauri::command]
pub fn intent_list(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> Result<Vec<IntentSnapshot>, IpcError> {
    commands::intent::intent_list(&state)
}

#[tauri::command]
pub fn intent_snapshot(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    intent_id: IntentId,
) -> Result<IntentSnapshot, IpcError> {
    commands::intent::intent_snapshot(&state, intent_id)
}

/// Answers the prompt, running the chosen action here or sending it to the device that was chosen.
///
/// ★ `(async)`: a remote action is awaited on this call for up to `REMOTE_RESULT_WAIT`, and a plain
/// `#[tauri::command]` would spend that time on the thread that received the invoke — the window's. Tauri's
/// `(async)` on a **synchronous** function puts the body on the sync threadpool instead, which is the same move
/// `devices_scan_start` makes for the scan window and for the same reason: the window has to keep painting while
/// the network is asked.
#[tauri::command(async)]
pub fn intent_decide(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    intent_id: IntentId,
    decision: IntentDecisionDto,
) -> Result<(), IpcError> {
    commands::intent::intent_decide(&state, intent_id, &decision)
}

/// Withdraws an intent, telling a device that is running it if there is one.
///
/// `(async)` for the dial the withdrawal may have to make, not for an answer: a cancellation is not replied to.
#[tauri::command(async)]
pub fn intent_cancel(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    intent_id: IntentId,
) -> Result<(), IpcError> {
    commands::intent::intent_cancel(&state, intent_id)
}

pub(super) fn dispatch(invoke: tauri::ipc::Invoke<tauri::Wry>) -> bool {
    let dispatch: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        intent_dispatch,
        intent_list,
        intent_snapshot,
        intent_decide,
        intent_cancel
    ];
    dispatch(invoke)
}
