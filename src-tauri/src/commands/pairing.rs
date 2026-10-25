//! App adapter for the shared restricted first-pairing service.
//!
//! Explicit imports approve the intended certificate received through a trusted out-of-band path.
//! No token or discovery metadata can authorize a peer; the shared service owns durable consent.

use std::sync::Arc;

use synra_core::peer_trust::{PairingService, PairingServiceError};
use synra_ipc::IpcError;
use synra_protocol::ids::{DeviceId, RequestId};
use synra_protocol::pairing::PairingSnapshot;
use synra_protocol::{ErrorCode, session::SessionTimeouts};

use crate::state::AppState;

/// Exports this enrolled device's public certificate invitation.
pub fn pairing_invite(state: &AppState) -> Result<String, IpcError> {
    service(state)?
        .invitation()
        .map_err(|error| refusal(&error))
}

/// Explicitly approves an imported invitation and opens a bounded restricted listener.
pub fn pairing_listen(
    state: &AppState,
    device_id: &str,
    invitation: &str,
) -> Result<PairingSnapshot, IpcError> {
    let peer = device(device_id)?;
    service(state)?
        .listen(
            peer,
            invitation,
            std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
            SessionTimeouts::default().pairing,
        )
        .map_err(|error| refusal(&error))
}

/// Explicitly approves a responder's certificate and connects using discovery only for routing.
pub fn pairing_connect(
    state: &AppState,
    device_id: &str,
    reply: &str,
) -> Result<PairingSnapshot, IpcError> {
    let peer = device(device_id)?;
    let service = service(state)?;
    // Discovery writes the app's seen-device table, not the outbound host's in-memory table.
    let seen = state.db().seen_devices().map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })?;
    let address = seen
        .iter()
        .find(|row| row.device_id == peer.to_string())
        .and_then(|row| row.source.parse::<std::net::SocketAddr>().ok())
        .ok_or_else(|| {
            IpcError::unknown(ErrorCode::DeviceOffline).with_params(
                serde_json::json!({"reason": "the selected device has no discovery route"}),
            )
        })?;
    service
        .connect(peer, reply, address, SessionTimeouts::default().pairing)
        .map_err(|error| refusal(&error))
}

/// Reads runtime progress, including a retained terminal result; never recovers sockets from disk.
pub fn pairing_status(state: &AppState) -> Result<Option<PairingSnapshot>, IpcError> {
    Ok(service(state)?.snapshot())
}

/// Cancels exactly one local generation before it commits its durable authorization.
pub fn pairing_cancel(state: &AppState, request_id: &str) -> Result<(), IpcError> {
    let id = request_id.parse::<RequestId>().map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"reason": error.to_string()}))
    })?;
    service(state)?.cancel(id).map_err(|error| refusal(&error))
}

/// Copies the shared service under a short host lock; all IO happens after the lock is released.
fn service(state: &AppState) -> Result<Arc<PairingService>, IpcError> {
    state.host.lock().pairing_service().map_err(|error| {
        IpcError::unknown(ErrorCode::NotPaired)
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })
}

fn device(value: &str) -> Result<DeviceId, IpcError> {
    value
        .parse()
        .map_err(|error: synra_protocol::ids::IdError| {
            IpcError::unknown(ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"reason": error.to_string()}))
        })
}

fn refusal(error: &PairingServiceError) -> IpcError {
    IpcError::unknown(error.code()).with_params(serde_json::json!({"detail": error.to_string()}))
}

/// Bridges shared task progress to the app's one sequenced kernel event stream.
#[cfg(feature = "tauri-shell")]
pub fn watch(app: tauri::AppHandle, cancel: &synra_core::supervisor::Cancellation) {
    use tauri::Manager as _;

    let cancel = cancel.clone();
    std::thread::Builder::new()
        .name("synra-pairing-watch".to_owned())
        .spawn(move || {
            let mut previous = None;
            while !cancel.is_cancelled() {
                let state = app.state::<Arc<AppState>>();
                if let Ok(Some(snapshot)) = pairing_status(&state)
                    && previous.as_ref() != Some(&snapshot)
                {
                    previous = Some(snapshot.clone());
                    state.publish(synra_ipc::event::KernelEvent::PairingUpdated(Box::new(
                        snapshot,
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        })
        .ok();
}

#[cfg(test)]
mod tests;
