#![allow(clippy::needless_pass_by_value)]

use synra_ipc::IpcError;

use crate::{commands, state::AppState};

/// Records that a plugin page has drawn, after checking the session it was given.
///
/// The label is the page's, not the plugin's: two pages of one plugin are two WebViews and two sessions, and a command
/// that took a plugin id could not tell them apart.
#[tauri::command]
pub fn plugin_ui_ready(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    label: String,
) -> Result<(), IpcError> {
    commands::plugin_ui::plugin_ui_ready(&state, &label)
}

/// Answers one call from a plugin page.
///
/// `(async)` because this is the one command whose operations may do I/O — a network fetch, a transfer — and the
/// plugin page that called it is not the window that must keep painting.
#[tauri::command(async)]
pub fn plugin_ui_rpc(
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    token: String,
    method: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    commands::plugin_ui::plugin_ui_rpc_arc(state.inner(), &token, &method, &args)
}

/// Opens a plugin's page in its own WebView, under its own origin.
///
/// ★ The contract declares it now (`plugin_ui_open` => PluginUi, **Host**), which is what makes registering it legal:
/// `registration.test.ts` refuses a shell that registers a name the kernel's table does not carry.
/// # What is deliberately *not* here
///
/// Every decision is in [`commands::plugin_ui::open_request`] — the address, the label, the token, the record in
/// `hosted_ui` — because those are values a test can assert. What is left is the one call no test in this repository
/// can make: `WebviewWindowBuilder`.
///
/// Async IPC dispatch keeps Windows WebView2 construction off the synchronous
/// command thread, where it otherwise deadlocks after creating an empty window.
#[tauri::command]
pub async fn plugin_ui_open(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    plugin_id: String,
) -> Result<(), IpcError> {
    crate::plugin_window::open(&app, state.inner(), &plugin_id)
}

pub(super) fn dispatch(invoke: tauri::ipc::Invoke<tauri::Wry>) -> bool {
    let dispatch: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool =
        tauri::generate_handler![plugin_ui_open, plugin_ui_ready, plugin_ui_rpc];
    dispatch(invoke)
}
