//! Platform window creation and lifetime around the testable plugin session reservation.
use crate::{AppState, commands};
use std::sync::Arc;
use synra_ipc::IpcError;

/// Open one authorized plugin page, or bring its existing document forward.
pub fn open(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    plugin_id: &str,
) -> Result<(), IpcError> {
    use tauri::Manager as _;

    let request = match commands::plugin_ui::open_request(state, plugin_id)? {
        commands::plugin_ui::OpenDecision::AlreadyOpen { label } => {
            if let Some(window) = app.get_window(&label) {
                let _ = window.show();
                let _ = window.set_focus();
            }
            return Ok(());
        }
        commands::plugin_ui::OpenDecision::Open(request) => request,
    };
    for label in &request.superseded {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.destroy();
        }
    }
    let failed = |error: String| {
        commands::plugin_ui::forget_window(state, &request.label);
        IpcError::unknown(synra_protocol::ErrorCode::Internal)
            .with_params(serde_json::json!({ "reason": error }))
    };
    let url = request
        .url
        .parse::<tauri::Url>()
        .map_err(|error| failed(error.to_string()))?;
    // ★ The address the WebView actually received, not the string `open_request` built. A navigation candidate is a
    // parsed `Url` too, and comparing the two normalised forms is what makes the first load and a hash route pass
    // while every other destination is refused.
    let opened = url.as_str().to_owned();
    let parent = app
        .get_webview_window("main")
        .ok_or_else(|| failed("the main window is unavailable".to_owned()))?;
    let builder = tauri::WebviewWindowBuilder::new(
        &parent,
        &request.label,
        tauri::WebviewUrl::CustomProtocol(url),
    )
    .initialization_script(&request.script)
    .use_https_scheme(true)
    // ★ The page may not navigate away. It holds this plugin's token and grants, and the script is attached to
    // the WebView rather than to an address — so another page, another origin or an external scheme would keep
    // this page's authority while showing something the opener never chose.
    .on_navigation(move |candidate| commands::plugin_ui::stays_on_page(candidate.as_str(), &opened))
    // And `window.open` is denied outright: a second window has no initialization script, so it would be a page
    // with no token — and on some platforms it would be a browser window the plugin opened, which is a decision
    // the platform capability exists to make.
    .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny);
    // MainActivity is singleTask: reusing it replaces the host page and leaves no back stack.
    #[cfg(target_os = "android")]
    let builder = builder.activity_name("PluginActivity");
    let window = builder.build().map_err(|error| {
        // The reservation `open_request` made must not outlive a window that was never built: otherwise every
        // failed open would leave the plugin permanently "already open" with no window to focus.
        failed(error.to_string())
    })?;

    // ★ Closing the window ends the session: the token table is the page's authority, and an entry that outlives the
    // page would let a closed window keep answering. `Destroyed` rather than `CloseRequested` because a close can be
    // cancelled, and a cancelled close must keep the page it kept.
    let state = std::sync::Arc::clone(state);
    let label = request.label.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Focused(false)) {
            let entry = state.hosted_ui.lock().get(&label).cloned();
            if let Some(entry) = entry {
                state.input.controller.close_plugin(&entry.plugin_id);
            }
        }
        if matches!(event, tauri::WindowEvent::Destroyed) {
            commands::plugin_ui::forget_window(&state, &label);
        }
    });
    Ok(())
}
