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

#[cfg(test)]
mod tests;
