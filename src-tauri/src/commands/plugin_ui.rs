//! What happens when a plugin page says it has drawn.
//!
//!
//! # Why a label and not a token
//!
//! `UiSessionRegistry::validate` needs three things: the nonce, the **claiming plugin** and its running version. A token is
//! a nonce — it says "some page holds this", not "this page is plugin X" — so the page cannot prove its own identity by
//! presenting one. The window that opened the WebView is the only thing that knows which plugin the page belongs to, so it
//! records that in [`hosted_ui`](crate::state::AppState::hosted_ui) and this command looks the label up.
//!
//! # What it refuses, and what it deliberately does not say
//!
//! A label nobody registered, and a token the registry will not validate, are both refusals — and both carry the same code.
//! Telling a caller which of the two it hit would let a page probe for labels that exist, which is the enumeration a token
//! is supposed to prevent.
//!/

use crate::state::AppState;
use synra_ipc::IpcError;
use synra_protocol::ErrorCode;

/// Records that a plugin page has mounted, after checking it holds a live session.
///
/// # Errors
///
/// [`ErrorCode::CapabilityDenied`] when the label is not one this shell hosted, or when the registry refuses the token —
/// see the module docs for why the two are not distinguished. [`ErrorCode::Internal`] is not refused here: validation does
/// not touch storage.
pub fn plugin_ui_ready(state: &AppState, label: &str) -> Result<(), IpcError> {
    let refused = || {
        IpcError::unknown(ErrorCode::CapabilityDenied)
            .with_params(serde_json::json!({"reason": "this page holds no live session"}))
    };

    // The three facts are **copied out** and the table's lock released before validation: `validate` takes the registry's
    // own lock, and holding two locks while a third party could take them in the other order is how a deadlock is built.
    // The window between the copy and the write is safe because the only thing that removes an entry is the window that
    // created it, and that same window is the one blocked on this command.
    let copy = {
        let hosted = state.hosted_ui.lock();
        hosted
            .get(label)
            .map(|entry| (entry.nonce, entry.plugin_id.clone(), entry.version.clone()))
    };
    let Some((nonce, plugin_id, version)) = copy else {
        return Err(refused());
    };

    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    state.ui_sessions.validate(nonce, &plugin_id, &version, now).map_err(|_| refused())?;

    if let Some(entry) = state.hosted_ui.lock().get_mut(label) {
        entry.ready = true;
    }
    Ok(())
}

/// The script a plugin page is started with: its identity, and the token `ready()` will present.
///
/// # Why the token is a **string**
///
/// Because it is a `u128`, and JavaScript numbers are exact only to 2^53. A nonce that silently loses its low digits is a
/// token that validates for nobody, and the failure would look like "the page never became ready" rather than like a
/// rounding error.
///
/// # Why `Object.freeze`
///
/// The page runs someone else's code. A mutable global is one a script can rewrite before the injected client reads it, so
/// the object is frozen at the moment it is created and the client closes over the values rather than re-reading them.
///
/// # Why the label is safe to interpolate
///
/// Tauri's WebView labels are its own identifiers (alphanumeric, `-`, `_`, `/`, `:`); this function is called with one the
/// shell created, never with anything a page supplied. The token is a number formatted here. Nothing else goes in.
#[must_use]
pub fn initialization_script(label: &str, token: u128) -> String {
    format!(
        "Object.freeze(window.__SYNRA_PLUGIN__ = Object.freeze({{ label: {label:?}, token: {:?} }}));",
        token.to_string()
    )
}

/// What the shell needs in order to open one plugin page.
///
/// The parts are separated from the Tauri call on purpose: a URL, a label and a script are values a test can assert, and
/// `WebviewWindowBuilder` is a call no test here can make. Everything that could be wrong in an interesting way is in this
/// struct; what is left in `shell.rs` is five lines of plumbing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    /// The WebView's label, and the key `plugin_ui_ready` is called with.
    pub label: String,
    /// The page's address, under the plugin origin: `synra-plugin://<plugin-id>/<page>`.
    pub url: String,
    /// The initialization script carrying the token and the label.
    pub script: String,
}

/// Prepares the page for one plugin, and records who it belongs to.
///
/// # Errors
///
/// [`ErrorCode::PluginNotInstalled`] when there is no manifest to read, [`ErrorCode::Unsupported`] when the plugin has no
/// UI half. Both are refusals a caller can act on: the first says "install it", the second says "this one has no window".
pub fn open_request(state: &AppState, plugin_id: &str) -> Result<OpenRequest, IpcError> {
    use synra_protocol::ErrorCode;

    let id = super::plugins::parse(plugin_id)?;
    let manifest = super::plugins::read_manifest(state, &id.to_string()).ok_or_else(|| {
        IpcError::unknown(ErrorCode::PluginNotInstalled)
            .with_params(serde_json::json!({"pluginId": plugin_id}))
    })?;
    let Some(ui) = manifest.runtime.ui.as_ref() else {
        return Err(IpcError::unknown(ErrorCode::Unsupported)
            .with_params(serde_json::json!({"pluginId": plugin_id, "reason": "it has no UI"})));
    };

    // ★ The page key is the manifest's default page when it names one, and the entry's file stem otherwise: the address
    // shape is `synra-plugin://<plugin-id>/<page>` and `entry` is a path inside the bundle (`dist/index.html`), which is
    // not a page key.
    let page = ui.default_page.clone().unwrap_or_else(|| {
        std::path::Path::new(&ui.entry)
            .file_stem()
            .map_or_else(|| "index".to_owned(), |stem| stem.to_string_lossy().into_owned())
    });
    let label = format!("plugin-{}", id.as_str().replace('/', "__"));
    let url = format!("synra-plugin://{}/{page}", id.as_str());

    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    let nonce = state.ui_sessions.issue_placeholder(
        &id,
        manifest.version.clone(),
        synra_capability::CapabilitySet::new(),
        now,
    );
    state.hosted_ui.lock().insert(
        label.clone(),
        crate::state::HostedUi { plugin_id: id, version: manifest.version, nonce, ready: false },
    );

    let script = initialization_script(&label, nonce);
    Ok(OpenRequest { label, url, script })
}

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
    let content_type = synra_plugin_ui::artifact::content_type_for(path).unwrap_or("application/octet-stream");
    Ok((bytes, content_type))
}

/// Splits a `synra-plugin://` address into the plugin it names and the path inside its bundle.
///
/// # Why this is not a `Url` parse
///
/// A plugin id contains a slash (`@acme/hello`), and the address is `synra-plugin://<plugin-id>/<path>` — so a URL parser
/// would read `@acme` as the host and `/hello/index.html` as the path, splitting the id in the wrong place. The split is
/// therefore made against the ids the host actually opened: the **longest** one that prefixes the address wins, and what
/// follows is the path.
///
/// Longest rather than first-match because two ids can share a prefix (`@acme/hello` and `@acme/hello-world`): first-match
/// would serve one plugin's files under the other's name.
#[must_use]
pub fn split_ui_url<'a>(url: &str, known: &'a [synra_protocol::ids::PluginId]) -> Option<(&'a synra_protocol::ids::PluginId, String)> {
    const SCHEME: &str = "synra-plugin://";
    let rest = url.strip_prefix(SCHEME)?;
    // A query or fragment is not part of a file path, and a cache-busting `?v=1` is common enough to allow.
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);

    let mut best: Option<(&'a synra_protocol::ids::PluginId, String)> = None;
    for id in known {
        let Some(after) = rest.strip_prefix(id.as_str()) else {
            continue;
        };
        let Some(path) = after.strip_prefix('/') else {
            continue;
        };
        let better = best.as_ref().is_none_or(|(winner, _)| id.as_str().len() > winner.as_str().len());
        if better {
            best = Some((id, path.to_owned()));
        }
    }
    best
}

#[cfg(test)]
mod tests;
