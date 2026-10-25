//! App identity maintenance delegates all replacement, revocation and cleanup rules to core.

use crate::state::AppState;
use synra_ipc::IpcError;
use synra_protocol::credentials::{
    CredentialCleanupReport, CredentialRepairReport, CredentialSnapshot,
};

/// Reads public enrollment and this process's captured authentication identity.
pub fn identity_credentials(state: &AppState) -> Result<CredentialSnapshot, IpcError> {
    let device = state.identity.lock().device_id;
    let runtime = state.host.lock().credential_fingerprint();
    synra_core::device_credentials::credential_snapshot(&state.db, device, runtime).map_err(
        |error| {
            let report = if matches!(
                error,
                synra_core::device_credentials::CredentialError::MissingRegistration
            ) {
                IpcError::new(error.code(), "error.identityHistoryMissing")
                    .unwrap_or_else(|_| IpcError::unknown(error.code()))
            } else {
                IpcError::unknown(error.code())
            };
            report.with_params(serde_json::json!({"detail": error.to_string()}))
        },
    )
}

/// Explicitly replaces the displayed certificate and revokes every previous pairing.
pub fn identity_repair(
    state: &AppState,
    expected_fingerprint: &str,
) -> Result<CredentialRepairReport, IpcError> {
    let device = state.identity.lock().device_id;
    synra_core::device_credentials::repair_native_identity(
        &state.db,
        device,
        expected_fingerprint,
        synra_store::sqlite::now_ms().max(0) as u64 / 1000,
    )
    .map_err(|error| {
        IpcError::unknown(error.code())
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })
}

/// Retries retired-key deletion independently of a successful replacement.
pub fn identity_cleanup(state: &AppState) -> Result<CredentialCleanupReport, IpcError> {
    synra_core::device_credentials::cleanup_native_identity(&state.db).map_err(|error| {
        IpcError::unknown(error.code())
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })
}

#[cfg(test)]
mod tests;
