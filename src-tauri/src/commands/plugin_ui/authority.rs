//! A nonce authorizes only the committed plugin authority it was issued against.

use crate::{AppState, state::HostedUi};
use synra_ipc::IpcError;
use synra_plugin_ui::ValidatedSession;
use synra_protocol::ErrorCode;

pub(super) fn refused() -> IpcError {
    IpcError::unknown(ErrorCode::CapabilityDenied)
        .with_params(serde_json::json!({"reason":"this page holds no live session"}))
}

pub(super) fn validate(state: &AppState, entry: &HostedUi) -> Result<ValidatedSession, IpcError> {
    let current = state
        .db()
        .plugin_authority(entry.plugin_id.as_str())
        .map_err(super::internal)?;
    if !entry.authority.permits_ui() || current != Some(entry.authority) {
        revoke(state, entry.nonce);
        return Err(refused());
    }
    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    let session = state
        .ui_sessions
        .validate(entry.nonce, &entry.plugin_id, &entry.version, now)
        .map_err(|_| refused());
    if session.is_err() {
        revoke(state, entry.nonce);
    }
    session
}

pub(super) fn revoke(state: &AppState, nonce: u128) {
    state.ui_sessions.revoke(nonce);
    state.uploads.revoke(nonce);
    #[cfg(any(feature = "tauri-shell", test))]
    state.plugin_events.close(nonce);
}
