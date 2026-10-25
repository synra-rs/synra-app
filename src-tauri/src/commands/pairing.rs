//! Pairing, from both ends.
//!
//! # What pairing is, in one paragraph
//!
//! Two devices that have never met agree on a secret neither of them knew: this device asks, the other person accepts, and the
//! accepting side issues a token that a later link must present. The user compares a short code on both screens, because a
//! device on the same network could otherwise ask in place of the one the user meant.
//!
//! # Why the desk lives in the state and not here
//!
//! Because an incoming request arrives on a socket thread and is answered from a command — different threads, no connection
//! between them. [`AppState::pairing`] is the one object both can see, and the answer travels back through the channel the desk
//! holds rather than through the connection that asked.
//!
//! # What is deliberately absent
//!
//! Any notion of "trusted on first use". A device that is not paired can ask, and until a person accepts it stays a request.
//! There is no path here that pairs without an answer, and that is the whole security argument of the feature.

use synra_core::pairing::PairingRecord;
use synra_ipc::IpcError;
use synra_protocol::ErrorCode;
use synra_protocol::ids::{DeviceId, RequestId, TraceId};
use synra_protocol::session::{PairResponse, PeerProfile, PlatformKind};

use crate::state::AppState;

/// Asks a discovered device to pair, and records it if the other person accepts.
///
/// # What this does, in order
///
/// Resolves the address the scan recorded, dials, sends the request over the new link, and **waits on that link** for the answer —
/// the dialer is the one holding the connection, so the reply arrives where the question left. A refusal is reported with the
/// peer's own reason rather than as a timeout, because "they said no" and "nobody answered" are different things to a user.
///
/// # Errors
///
/// [`ErrorCode::Unsupported`] when the identifier is unknown or has no address, [`ErrorCode::DeviceNotFound`] when the device is
/// not in the scan table at all, and [`ErrorCode::TransferFailed`] for a dial or a peer that refused.
pub fn pairing_request(state: &AppState, device_id: &str) -> Result<RequestId, IpcError> {
    let to = device_id.parse::<DeviceId>().map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"deviceId": device_id, "reason": error.to_string()}))
    })?;

    // ★ A device cannot pair with itself. The check is here rather than in the interface because this device hears its own
    // announcement, so it appears in its own scan table — and a self-pairing would write a token nobody else ever holds.
    if to == state.identity.lock().device_id {
        return Err(IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"deviceId": device_id, "reason": "a device cannot pair with itself"})));
    }

    // ★ The address the scan recorded, which is the only place a device nobody has paired is described. It lives in `source`
    // rather than a column of its own: the discovery code writes `candidate.address.to_string()` there.
    let address = {
        let seen = state.db().seen_devices().map_err(internal)?;
        seen.iter()
            .find(|row| row.device_id == device_id)
            .map(|row| row.source.clone())
            .ok_or_else(|| {
                IpcError::unknown(ErrorCode::NotPaired).with_params(
                    serde_json::json!({"deviceId": device_id, "reason": "it has not been seen"}),
                )
            })?
    };
    let address = address.parse::<std::net::SocketAddr>().map_err(|error| {
        IpcError::unknown(ErrorCode::Unsupported)
            .with_params(serde_json::json!({"address": address, "reason": error.to_string()}))
    })?;

    let identity = state.identity.lock().clone();
    let ours = profile_of(&identity);
    let now_ms = unix_millis();
    let window = synra_protocol::session::SessionTimeouts::default().pairing;

    let parts = state.host.lock().dialling_parts();
    let dialled =
        synra_core::host::dial_peer(&parts.0, parts.1, address, to, now_ms).map_err(|error| {
            IpcError::unknown(ErrorCode::TransferFailed).with_params(
                serde_json::json!({"deviceId": device_id, "detail": error.to_string()}),
            )
        })?;
    let mut link = dialled
        .into_link()
        .with_metrics(std::sync::Arc::clone(&state.metrics));

    let request_id = RequestId::new();
    let envelope = synra_core::messaging::pair_request(
        identity.device_id,
        to,
        &ours,
        TraceId::new(),
        synra_core::host::millis_reading(now_ms),
    )
    .map_err(internal)?;
    link.send(&envelope).map_err(|error| {
        IpcError::unknown(ErrorCode::TransferFailed)
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })?;

    // ★ A deadline on the read, so a peer that never answers produces "they did not answer" instead of a command that hangs for
    // ever. The window is the protocol's own pairing window rather than a number chosen here: two different windows would mean a
    // prompt answered just inside one and just outside the other.
    link.set_read_timeout(Some(window));
    let answer = wait_for_answer(&mut link)?;

    if !answer.accepted {
        return Err(IpcError::unknown(ErrorCode::NotPaired).with_params(
            serde_json::json!({"deviceId": device_id, "reason": answer.reason.unwrap_or_else(|| "declined".to_owned())}),
        ));
    }
    let token = answer.issued_token().ok_or_else(|| {
        IpcError::unknown(ErrorCode::TransferFailed).with_params(
            serde_json::json!({"deviceId": device_id, "reason": "it accepted but issued no token"}),
        )
    })?;

    let display_name = {
        let db = state.db();
        db.seen_devices()
            .map_err(internal)?
            .iter()
            .find(|row| row.device_id == device_id)
            .map_or_else(|| device_id.to_owned(), |row| row.display_name.clone())
    };
    state
        .db()
        .pair_device(&synra_store::Paired {
            device_id: device_id.to_owned(),
            display_name,
            platform: platform_name(ours.platform).to_owned(),
            // The profile carries no feature list, so this records "none agreed" rather than inventing one. A pairing is about
            // recognising a device, not about what it may do — that is decided by capabilities, later and separately.
            features: "[]".to_owned(),
            link_token: token.to_owned(),
            address: Some(address.to_string()),
            paired_at_ms: synra_core::host::millis_reading(now_ms),
        })
        .map_err(internal)?;

    state.publish(synra_ipc::event::KernelEvent::PairingResolved { request_id });
    // ★ The id is the answer, as 08's signature says (`Result<RequestId, IpcError>`). The page ignores it today, but a
    // command declared `One` that returns nothing made the contract's own arity a lie.
    Ok(request_id)
}

/// Reads the link until the answer to a pairing request arrives.
///
/// Blocking and bounded by the link's read deadline. Anything that is not the answer is skipped rather than refused: the peer may
/// send an acknowledgement first, and a pairing that failed because of traffic ordering would be a puzzle for the user to solve.
fn wait_for_answer(link: &mut synra_core::link::PeerLink) -> Result<PairResponse, IpcError> {
    loop {
        match link.receive_inbound() {
            Ok(synra_core::link::Inbound::Envelope(envelope))
                if envelope.event.as_str() == synra_protocol::event::events::PAIR_RESPONSE =>
            {
                let Some(json) = envelope.payload_str() else {
                    return Err(IpcError::unknown(ErrorCode::TransferFailed).with_params(
                        serde_json::json!({"reason": "the answer carried no payload"}),
                    ));
                };
                return serde_json::from_str::<PairResponse>(json).map_err(internal);
            }
            // The answer is the only thing this side is waiting for; other traffic on a fresh link is not an error.
            Ok(_) => continue,
            Err(error) => {
                return Err(IpcError::unknown(ErrorCode::TransferFailed).with_params(
                    serde_json::json!({"reason": format!("the peer did not answer: {error}")}),
                ));
            }
        }
    }
}

/// Answers a pairing request that is waiting, and records the pairing when the answer is yes.
///
/// # Errors
///
/// [`ErrorCode::Unsupported`] when nothing is waiting under that identifier, and [`ErrorCode::Internal`] when the pairing cannot
/// be written — in which case the prompt stays on screen, because the write is attempted **inside** the desk's own decision.
pub fn pairing_respond(state: &AppState, request_id: &str, accept: bool) -> Result<(), IpcError> {
    let id = request_id.parse::<RequestId>().map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"requestId": request_id, "reason": error.to_string()}))
    })?;

    let now = unix_millis();
    let db = state.db();
    let written = state.pairing.answer(id, accept, |record: &PairingRecord| {
        db.pair_device(&synra_store::Paired {
            device_id: record.device_id.to_string(),
            display_name: record.display_name.clone(),
            platform: platform_name(record.platform).to_owned(),
            features: "[]".to_owned(),
            link_token: record.link_token.clone(),
            address: Some(record.address.clone()),
            paired_at_ms: synra_core::host::millis_reading(now),
        })
        .map_err(|error| error.to_string())
    });
    drop(db);

    if written.is_none() {
        // ★ `NotPaired`, not `Unsupported`: the caller's screen was right a moment ago and the prompt expired or was answered
        // elsewhere. "Nothing is waiting" is a state of the world the user can retry, not a capability this build lacks.
        return Err(IpcError::unknown(ErrorCode::NotPaired).with_params(
            serde_json::json!({"requestId": request_id, "reason": "nothing is waiting"}),
        ));
    }
    state.publish(synra_ipc::event::KernelEvent::PairingResolved { request_id: id });
    Ok(())
}

/// Refuses a waiting request. The link stays up, so the dialer may ask again.
///
/// # Errors
///
/// As [`pairing_respond`].
pub fn pairing_cancel(state: &AppState, request_id: &str) -> Result<(), IpcError> {
    pairing_respond(state, request_id, false)
}

/// Publishes a prompt for every request that appears on the desk.
///
/// # Why polling
///
/// Because the desk has no subscription: it is a table plus a channel per entry, and the channel is what the *connection* waits on.
/// A thread turning over four times a second is the smallest thing that turns an arrival into an event, and the alternative — a
/// callback from the socket thread into Tauri — would put the interface in the path of a handshake.
#[cfg(feature = "tauri-shell")]
pub fn watch(app: tauri::AppHandle, cancel: &synra_core::supervisor::Cancellation) {
    use tauri::Manager as _;

    let cancel = cancel.clone();
    std::thread::Builder::new()
        .name("synra-pairing-watch".to_owned())
        .spawn(move || {
            let mut announced: std::collections::HashSet<RequestId> =
                std::collections::HashSet::new();
            while !cancel.is_cancelled() {
                let state = app.state::<std::sync::Arc<AppState>>();
                let prompts: Vec<synra_ipc::dto::PairingPrompt> = state
                    .pairing
                    .prompts()
                    .into_iter()
                    .map(|pending| synra_ipc::dto::PairingPrompt {
                        request_id: pending.request_id.to_string(),
                        device_id: pending.request.profile.device_id,
                        display_name: pending.request.profile.display_name.clone(),
                        platform: platform_name(pending.request.profile.platform).to_owned(),
                        address: pending.address.clone(),
                        verification_code: pending.verification_code.clone(),
                        expires_at_ms: pending.expires_at_ms,
                    })
                    .collect();
                // Forget ids that are no longer waiting, so a second request from the same device is announced again.
                announced.retain(|id| {
                    prompts
                        .iter()
                        .any(|prompt| prompt.request_id == id.to_string())
                });
                for prompt in prompts {
                    let Ok(id) = prompt.request_id.parse::<RequestId>() else {
                        continue;
                    };
                    if announced.insert(id) {
                        state.publish(synra_ipc::event::KernelEvent::PairingRequested(Box::new(
                            prompt,
                        )));
                    }
                }
                // The guard is dropped at the end of this block; the sleep is outside the borrow.
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        })
        .ok();
}

/// What this device says about itself when it asks.
fn profile_of(identity: &synra_core::identity::Identity) -> PeerProfile {
    PeerProfile {
        device_id: identity.device_id,
        display_name: identity.display_name.clone(),
        platform: platform_of(&identity.platform),
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

/// The identity stores the platform as a string; the wire wants the enum.
fn platform_of(platform: &str) -> PlatformKind {
    match platform.to_ascii_lowercase().as_str() {
        "android" => PlatformKind::Android,
        "ios" | "macos" | "mac" => PlatformKind::Ios,
        _ => PlatformKind::Desktop,
    }
}

/// The string the device table stores for a platform.
fn platform_name(platform: PlatformKind) -> &'static str {
    match platform {
        PlatformKind::Desktop => "desktop",
        PlatformKind::Android => "android",
        PlatformKind::Ios => "ios",
        // PlatformKind is #[non_exhaustive]: a platform this build does not know is reported by name, not guessed.
        _ => "unknown",
    }
}

/// Milliseconds since the epoch, as the wire's `i64`.
fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0_u64, |elapsed| elapsed.as_millis() as u64)
}

/// A database failure is this shell's problem, not the caller's: the peer did nothing wrong.
fn internal(error: impl std::fmt::Display) -> IpcError {
    IpcError::unknown(synra_protocol::ErrorCode::Internal)
        .with_params(serde_json::json!({"detail": error.to_string()}))
}

#[cfg(test)]
mod tests;
