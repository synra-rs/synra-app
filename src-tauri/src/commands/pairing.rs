//! App adapter for the shared restricted first-pairing service.
//!
//! Explicit imports approve the intended certificate received through a trusted out-of-band path.
//! No token or discovery metadata can authorize a peer; the shared service owns durable consent.

use std::sync::Arc;

use serde::Serialize;
use synra_core::peer_trust::{PairingService, PairingServiceError};
use synra_ipc::IpcError;
use synra_protocol::ids::{DeviceId, RequestId};
use synra_protocol::pairing::PairingSnapshot;
use synra_protocol::ErrorCode;

use crate::state::AppState;

/// One device asking to pair, as the person who has to decide about it needs to see it.
///
/// # Why these fields and not the whole `PendingPairing`
///
/// `PendingPairing` carries the link token and the whole `PairRequest` profile — machine state. What a person deciding
/// needs is: who is asking (a name and the full id, because names are not unique), where it came from (observed, not
/// claimed), the code to compare against the other screen, and how long is left.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPairingDto {
    pub request_id: String,
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    /// Where the request came from, as this side observed it. Empty when it arrived without an address.
    pub address: String,
    /// The short code the other device is showing, for a person to compare the two.
    pub verification_code: String,
    /// How long is left before this stops being worth deciding about. Zero means it is already past.
    pub expires_in_ms: i64,
}

/// Reads runtime progress, including a retained terminal result; never recovers sockets from disk.
pub fn pairing_status(state: &AppState) -> Result<Option<PairingSnapshot>, IpcError> {
    Ok(service(state)?.snapshot())
}

/// What this device is being asked to decide about, right now.
///
/// # Why this exists
///
/// `AppState.pairing` has held the desk for as long as the desk has existed, and nothing read it. A request from another
/// device arrives on a socket thread, the desk files it and parks that thread — and no code asked the only person who can
/// answer. The dialer waited out its window and was told the request was never answered.
///
/// This is the asking half; [`answer_pairing`] is the other.
pub fn pairing_pending(state: &AppState) -> Result<Vec<PendingPairingDto>, IpcError> {
    let now = synra_store::sqlite::now_ms();
    Ok(state
        .pairing
        .prompts()
        .into_iter()
        .map(|prompt| PendingPairingDto {
            request_id: prompt.request_id.to_string(),
            device_id: prompt.request.profile.device_id.to_string(),
            display_name: prompt.request.profile.display_name.clone(),
            platform: format!("{:?}", prompt.request.profile.platform),
            address: prompt.address.clone(),
            verification_code: prompt.verification_code.clone(),
            expires_in_ms: prompt.expires_at_ms.saturating_sub(now).max(0),
        })
        .collect())
}

/// Accepts or declines one waiting request, recording the pairing when it is accepted.
///
/// # Why the write is supplied here
///
/// The desk answers only after `write` has succeeded, and that order is the point: an acceptance that reached the dialer
/// while this side was still writing would be a promise the database might not keep. The desk owns that ordering; this
/// supplies the writer.
pub fn answer_pairing(state: &AppState, request_id: &str, accept: bool) -> Result<(), IpcError> {
    let id = request_id.parse::<RequestId>().map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"reason": error.to_string()}))
    })?;
    let db = Arc::clone(&state.db);
    let answered = state.pairing.answer(id, accept, move |record| {
        db.lock()
            .pair_device(&synra_store::sqlite::Paired {
                device_id: record.device_id.to_string(),
                display_name: record.display_name.clone(),
                platform: format!("{:?}", record.platform),
                features: "[]".to_owned(),
                link_token: record.link_token.clone(),
                address: Some(record.address.clone()).filter(|value| !value.is_empty()),
                paired_at_ms: synra_store::sqlite::now_ms(),
            })
            .map_err(|error| error.to_string())
    });
    match answered {
        // Nothing was waiting: it expired, or another command answered it first. Not a fault of the caller's — the list
        // they acted on is simply out of date, and a refresh corrects it.
        None => Err(IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"reason": "the request is no longer waiting"}))),
        Some(response) if response.accepted => Ok(()),
        // A decline is not a failure. The desk sent `accepted: false` back to the dialer, which is the outcome asked
        // for, so the command reports success and the person sees the prompt go away.
        Some(response) if response.reason.as_deref() == Some("declined") => Ok(()),
        // The write failed. The desk put the entry back, so the prompt is still there and can be tried again once the
        // cause is gone — the reason is shown rather than swallowed.
        Some(response) => Err(IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({
            "detail": response.reason.unwrap_or_else(|| "the pairing could not be recorded".to_owned())
        }))),
    }
}

/// What the person who pressed "pair" sees while the other device decides.
///
/// # Why the code is returned rather than looked up later
///
/// The six digits are derived from both device ids, so this side can show them the moment it starts — it does not have to
/// wait to be told. That is what makes the comparison possible at all: two screens, one number, checked by a person.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingPairingDto {
    pub device_id: String,
    pub display_name: String,
    /// The code this device is showing, for a person to compare against the other screen.
    pub verification_code: String,
    /// How long the answer is worth waiting for.
    pub expires_in_ms: i64,
}

/// Asks a device to pair, and reports the code to compare while it decides.
///
/// # Why this returns before the answer arrives
///
/// The request waits for a person on the other machine — up to [`synra_core::pairing::PATIENCE`]. An IPC command that
/// blocked for that long would freeze the window it was called from, so the dial runs on its own thread and this reports
/// only what is already known: who was asked, the code, and how long it is good for. A record is written when, and only
/// when, the other device accepts.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] when discovery has no current address for that device — a machine that has moved, or one that
/// was heard a while ago. [`ErrorCode::Internal`] when this device has no enrolled identity to present.
pub fn pairing_request(state: &AppState, device_id: &str) -> Result<OutgoingPairingDto, IpcError> {
    let peer = device_id.parse::<DeviceId>().map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"reason": error.to_string()}))
    })?;
    let (address, credentials, profile, local) = {
        let host = state.host.lock();
        let address = host.locate(peer).ok_or_else(|| {
            IpcError::unknown(ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"reason": "the device is not nearby"}))
        })?;
        let credentials = host.pairing_credentials().map_err(|error| {
            IpcError::unknown(ErrorCode::Internal)
                .with_params(serde_json::json!({"reason": error.to_string()}))
        })?;
        let identity = state.identity.lock();
        let profile = synra_protocol::session::PeerProfile {
            device_id: identity.device_id,
            display_name: identity.display_name.clone(),
            platform: match identity.platform.as_str() {
                "android" => synra_protocol::session::PlatformKind::Android,
                _ => synra_protocol::session::PlatformKind::Desktop,
            },
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        (address, credentials, profile, identity.device_id)
    };
    let display_name = profile.display_name.clone();
    // A second binding for the thread: the record it may write needs the name, and a `move` closure would take the one
    // this function still has to return.
    let recorded_name = display_name.clone();
    let code = synra_core::pairing::verification_code(local, peer);
    let db = Arc::clone(&state.db);
    std::thread::spawn(move || {
        let socket = match std::net::TcpStream::connect(address) {
            Ok(socket) => socket,
            Err(error) => {
                eprintln!("synra: the pairing request could not be sent: {error}");
                return;
            }
        };
        let request = synra_protocol::session::PairRequest { profile, user_hint: None };
        let answer = synra_core::pairing::transport::ask(socket, &credentials, peer, &request);
        let Ok(answer) = answer else {
            eprintln!("synra: the pairing request was not answered");
            return;
        };
        // A refusal is an answer, not a fault: the other person said no, and nothing is recorded.
        if !answer.accepted {
            return;
        }
        let Some(token) = answer.link_token.clone() else {
            return;
        };
        let _ = db.lock().pair_device(&synra_store::sqlite::Paired {
            device_id: peer.to_string(),
            display_name: recorded_name,
            platform: String::new(),
            features: "[]".to_owned(),
            link_token: token.to_owned(),
            address: Some(address.to_string()),
            paired_at_ms: synra_store::sqlite::now_ms(),
        });
    });
    Ok(OutgoingPairingDto {
        device_id: peer.to_string(),
        display_name,
        verification_code: code,
        expires_in_ms: i64::try_from(synra_core::pairing::PATIENCE.as_millis()).unwrap_or(i64::MAX),
    })
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
