use super::*;

/// One file out of a plugin's extracted UI bundle.
///
/// # What this is, and what it is not
///
/// It is the **policy** half of the `synra-plugin://` handler: which paths are legal, which content type a file gets, and
/// that the joined path is still inside the bundle. It is not the Tauri handler, because a handler needs a running
/// application and this needs a directory — so the interesting failure (a page asking for `../../identity.toml`) is a unit
/// test rather than a manual experiment.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] for a path the artifact rules refuse or one that escapes the root, and
/// [`ErrorCode::Internal`] when the file cannot be read. The two are different answers: the first is somebody's bug or an
/// attack, the second is a broken install.
pub fn ui_file(root: &std::path::Path, path: &str) -> Result<(Vec<u8>, &'static str), IpcError> {
    use synra_protocol::ErrorCode;

    synra_plugin_ui::artifact::check_path(path).map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"path": path, "reason": error.to_string()}))
    })?;

    let file = root.join(path);
    // ★ Belt **and** braces: `check_path` already refuses `..` and absolute paths, and this refuses a join that escaped
    // anyway. A path check that is the only guard is one refactor away from not being a guard.
    if !synra_store::is_within(root, &file) {
        return Err(IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"path": path, "reason": "it leaves the bundle"})));
    }

    let bytes = std::fs::read(&file).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"path": path, "reason": error.to_string()}))
    })?;
    // A file nobody said the type of is still a file: `application/octet-stream` makes the WebView download it rather than
    // guess, which is the honest default.
    let content_type =
        synra_plugin_ui::artifact::content_type_for(path).unwrap_or("application/octet-stream");
    Ok((bytes, content_type))
}

/// Resolves a package's URL-safe origin and a validated path inside its UI bundle.
#[must_use]
pub fn split_ui_url<'a>(
    url: &str,
    known: &'a [synra_protocol::ids::PluginId],
) -> Option<(&'a synra_protocol::ids::PluginId, String)> {
    origin::split(url, known)
}

/// The policy every plugin page is served with.
///
/// Tauri's configured CSP covers app assets; this custom protocol attaches the
/// plugin policy to its own responses (08 §550, 09 §416), independently of the host document.
/// `'unsafe-inline'` for scripts and styles is the design's own allowance — a plugin page is a bundle, not a host page.
/// Tauri IPC uses `ipc:` on native WebKit and the HTTPS localhost mapping selected
/// for these windows on Windows/Android. Neither source permits ordinary network IO.
#[cfg(feature = "tauri-shell")]
pub(crate) const PLUGIN_CSP: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; \
     style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; \
     connect-src ipc: https://ipc.localhost; \
     base-uri 'none'; form-action 'none'";

/// A successful page response, with the policy attached.
#[cfg(feature = "tauri-shell")]
pub(crate) fn document_response(
    bytes: Vec<u8>,
    content_type: &'static str,
) -> tauri::http::Response<std::borrow::Cow<'static, [u8]>> {
    use std::borrow::Cow;

    tauri::http::Response::builder()
        .status(200)
        .header(tauri::http::header::CONTENT_TYPE, content_type)
        .header("content-security-policy", PLUGIN_CSP)
        .body(Cow::Owned(bytes))
        .unwrap_or_else(|_| tauri::http::Response::new(Cow::Owned(Vec::new())))
}

/// Removes a page's session when its window is gone.
///
/// ★ A page's authority is its token, and the token is in a table the page cannot write. Until this runs, a closed window
/// leaves a live session behind: `plugin_ui_ready` and the protocol handler answer for a page that no longer exists, and a
/// second window with the same label would inherit the first one's nonce. Closing is the end of the session.
pub fn forget_window(state: &AppState, label: &str) {
    let entry = state.hosted_ui.lock().remove(label);
    if let Some(entry) = entry {
        state.input.controller.close_plugin(&entry.plugin_id);
        let _ = state.ui_sessions.revoke(entry.nonce);
    }
}

/// Serves resources only for the requesting document's live authority, including after updates.
pub fn page_resource(
    state: &AppState,
    label: &str,
    uri: &str,
) -> Result<(Vec<u8>, &'static str), IpcError> {
    let entry = state
        .hosted_ui
        .lock()
        .get(label)
        .cloned()
        .ok_or_else(authority::refused)?;
    authority::validate(state, &entry)?;
    let (_, path) =
        split_ui_url(uri, std::slice::from_ref(&entry.plugin_id)).ok_or_else(authority::refused)?;
    let root =
        synra_plugin_ui::artifact::bundle_dir(&state.paths.plugin_dir(entry.plugin_id.as_str()));
    ui_file(&root, &path)
}

/// Returns stale document labels; keep reservations until destruction so failed closes can be retried.
pub fn stale_windows(state: &AppState) -> Vec<String> {
    let snapshots: Vec<_> = state
        .hosted_ui
        .lock()
        .iter()
        .map(|(label, entry)| (label.clone(), entry.clone()))
        .collect();
    let mut labels = Vec::new();
    for (label, entry) in snapshots {
        if authority::validate(state, &entry).is_err() {
            labels.push(label);
        }
    }
    labels
}
