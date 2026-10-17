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

#[cfg(test)]
mod tests;
