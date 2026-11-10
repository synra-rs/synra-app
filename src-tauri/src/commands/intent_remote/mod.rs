//! Putting an intent's envelope on a link to the device that has to run it.
//!
//! # Why this exists
//!
//! `orchestrate::decide` answers a target that is not this device with an [`Outbound`] — "the envelope a host must put on
//! a link" — and until this module the shell refused to send it: `intent_decide` reported "this shell has no transport"
//! for a candidate on another device. That sentence was true when it was written and stopped being true when the shell
//! grew a listener and a dialler for transfers. The user-visible result of the stale refusal was a chooser that offered a
//! device the user could pair, showed it as online, and then failed the moment they picked it.
//!
//! The daemon has sent these envelopes since `runtime::commands::send_outbound`. This is the same exchange (send, read
//! **the one answer the same connection carries**, apply it) in the host that has a window.
//!
//! # Why the answer is read here rather than left to the listener
//!
//! Because it belongs to the question. An invocation is answered on the connection it arrived on, and the far side closes
//! it afterwards; a reply that went to whatever listener happened to be free would be a reply to nobody. The listener's
//! `INTENT_RESULT` path still exists and is still needed for the *late* answer — the one that arrives after the sender
//! gave up — but the answer inside the wait has to be read by the sender.
//!
//! # What a missing answer means
//!
//! Nothing is invented. A timeout returns `Ok(None)` and the intent stays `dispatching`, because the action may still be
//! running over there; the far side's late answer is applied by the listener. That limitation is the same one
//! [`synra_core::dispatch::REMOTE_RESULT_WAIT`] documents.

use std::time::Duration;

use synra_core::intent::remote::Outbound;
use synra_core::link::PeerLink;
use synra_ipc::IpcError;
use synra_protocol::ErrorCode;
use synra_protocol::envelope::Envelope;

use crate::state::AppState;

/// Dials the device the envelope names, sends it, and — when `wait` is given — reads the answer.
///
/// `wait` is `Some` for an invocation and `None` for a withdrawal. A withdrawal is not answered (there is no "I stopped"
/// reply in the protocol), so waiting for one would hold the caller for the whole timeout to learn what it already knows.
///
/// # Errors
///
/// [`ErrorCode::Unsupported`] when the device has no address this side has seen (from `address_of`),
/// [`ErrorCode::DeviceOffline`] when every dial attempt fails, [`ErrorCode::TransportNotOpen`] when the write or the read
/// fails, and [`ErrorCode::InvalidArgument`] when the peer answers with something that is not an intent result.
pub fn deliver(
    state: &AppState,
    outbound: &Outbound,
    wait: Option<Duration>,
) -> Result<Option<Envelope>, IpcError> {
    let address = crate::commands::transfers::address_of(state, outbound.to)?;
    let now_ms = crate::commands::transfers::unix_millis();
    let failed = |reason: String| {
        IpcError::unknown(ErrorCode::DeviceOffline)
            .with_params(serde_json::json!({"deviceId": outbound.to.to_string(), "reason": reason}))
    };

    // ★ Retried for the reason `synra_core::retry` documents: the ordinary way a dial fails is a listener that is still
    // closing the previous connection. One attempt would report a device the user can see as offline because it was busy
    // for a hundred milliseconds. The bound and the pauses are the kernel's, not a second copy of them.
    let parts = state.host.lock().dialling_parts();
    let dialled = synra_core::retry::with_retries(
        synra_core::retry::ATTEMPTS,
        |_| {
            synra_core::host::dial_peer(&parts.0, parts.1, address, outbound.to, now_ms)
                .map_err(|error| failed(format!("the dial failed: {error}")))
        },
        |attempt| {
            std::thread::sleep(Duration::from_millis(
                100 * (u64::try_from(attempt).unwrap_or(0) + 1),
            ));
        },
    )?;

    let mut link = dialled
        .into_link()
        // The byte counters live on the link; without this the shell would own a recorder no intent envelope ever
        // reached, and `bytes_sent_total` would count files but not actions.
        .with_metrics(std::sync::Arc::clone(&state.metrics));

    exchange_on(&mut link, outbound, wait)
}

/// The half that needs no socket: send, optionally read one answer, and say goodbye.
///
/// # Why the link is a parameter
///
/// Because this is the part with the decisions in it — what counts as an answer, what a timeout means, what is a protocol
/// mistake — and a test that had to dial could not run without a peer. A memory link drives it exactly as a socket does;
/// the dial above is the only part that has to be real.
fn exchange_on(
    link: &mut PeerLink,
    outbound: &Outbound,
    wait: Option<Duration>,
) -> Result<Option<Envelope>, IpcError> {
    // The half with the decisions in it is the kernel's, so the daemon answers "is this an answer?" the same way.
    synra_core::intent::remote::exchange(link, outbound, wait)
        .map_err(|error| IpcError::from_synra_error(&error))
}

/// How long a decision waits, decided by what the envelope is.
///
/// ★ Derived from the envelope rather than passed by each branch: an invocation has an answer to wait for and a
/// withdrawal does not. A caller that passed the wait by hand could wait eight seconds for a message the protocol never
/// answers, and the mistake would look like a slow network.
fn wait_for(outbound: &Outbound) -> Option<Duration> {
    (outbound.envelope.event.as_str() == synra_protocol::event::events::INTENT_INVOKE)
        .then_some(synra_core::dispatch::REMOTE_RESULT_WAIT)
}

/// [`deliver`] with the wait the envelope itself implies.
///
/// # Errors
///
/// As [`deliver`].
pub fn deliver_decided(
    state: &AppState,
    outbound: &Outbound,
) -> Result<Option<Envelope>, IpcError> {
    deliver(state, outbound, wait_for(outbound))
}

#[cfg(test)]
mod tests;
