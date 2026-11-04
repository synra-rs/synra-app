//! Resolve paired peers from current discovery before falling back to their last known endpoint.

use crate::state::{AppState, internal};
use std::net::SocketAddr;
use synra_core::SharedHost;
use synra_ipc::IpcError;
use synra_protocol::{DeviceId, ErrorCode};

pub(crate) struct Presence {
    pub(crate) online: bool,
    pub(crate) address: Option<String>,
    pub(crate) last_seen_ms: i64,
}

pub(crate) fn presence(host: &SharedHost, row: &synra_store::DeviceRow, now_ms: i64) -> Presence {
    let Ok(id) = row.device_id.parse::<DeviceId>() else {
        return Presence {
            online: false,
            address: row.address.clone(),
            last_seen_ms: row.last_seen_ms,
        };
    };
    let host = host.lock();
    let current = host.recent_address(id, now_ms);
    let seen = host
        .discovery()
        .get(id)
        .map_or(row.last_seen_ms, |peer| peer.last_seen_ms);
    Presence {
        online: !row.blocked
            && (current.is_some()
                || host.sessions().live_for(id).is_some_and(|session| {
                    session.heartbeat_deadline_ms() > synra_core::host::reading_millis(now_ms)
                })),
        address: current
            .map(|address| address.to_string())
            .or_else(|| row.address.clone()),
        last_seen_ms: row.last_seen_ms.max(seen),
    }
}

/// A paired and unblocked peer's listening endpoint, shared by sync, transfers and remote intents.
/// # Errors
/// Refuses unpaired/blocked peers, missing endpoints and unreadable stored addresses.
pub fn address_of(state: &AppState, to: DeviceId) -> Result<SocketAddr, IpcError> {
    resolve(&state.db, &state.host, to)
}

pub(crate) fn resolve(
    db: &std::sync::Arc<parking_lot::Mutex<synra_store::SynraDb>>,
    host: &SharedHost,
    to: DeviceId,
) -> Result<SocketAddr, IpcError> {
    let row = db
        .lock()
        .paired_devices()
        .map_err(internal)?
        .into_iter()
        .find(|row| row.device_id == to.to_string() && !row.blocked)
        .ok_or_else(|| {
            IpcError::unknown(ErrorCode::Unsupported).with_params(
                serde_json::json!({"deviceId": to, "reason": "it is not an approved peer"}),
            )
        })?;
    let current = host
        .lock()
        .recent_address(to, synra_store::sqlite::now_ms());
    if let Some(address) = current {
        return Ok(address);
    }
    let stored = row.address.ok_or_else(|| {
        IpcError::unknown(ErrorCode::Unsupported).with_params(
            serde_json::json!({"deviceId": to, "reason": "it has no listening address"}),
        )
    })?;
    stored.parse::<SocketAddr>().map_err(|error| {
        IpcError::unknown(ErrorCode::Unsupported)
            .with_params(serde_json::json!({"address": stored, "reason": error.to_string()}))
    })
}

#[cfg(test)]
mod tests;
