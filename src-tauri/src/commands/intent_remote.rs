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
mod tests {
    mod authenticated;
    use std::sync::Arc;

    use synra_core::intent::remote::{self, Invoke, InvokeResult};
    use synra_core::link::memory::MemoryStream;
    use synra_protocol::handshake::Agreed;
    use synra_protocol::ids::{DeviceId, RequestId, TraceId};

    use super::*;

    const LOCAL: &str = "11111111-1111-4111-8111-111111111111";
    const PEER: &str = "22222222-2222-4222-8222-222222222222";

    fn device(name: &str) -> DeviceId {
        name.parse().expect("a valid device id")
    }

    fn linked() -> (PeerLink, PeerLink) {
        let (here, there) = MemoryStream::pair();
        // ★ A production-shaped link rather than a permissive test one: `Flags::production()` is what the caller uses, and
        // a fixture with no timeouts would not exercise the deadline this module's whole wait depends on.
        (
            PeerLink::new(
                here,
                device(LOCAL),
                device(PEER),
                Agreed::default(),
                synra_core::peer::Flags::production(),
            ),
            PeerLink::new(
                there,
                device(PEER),
                device(LOCAL),
                Agreed::default(),
                synra_core::peer::Flags::production(),
            ),
        )
    }

    fn invitation(request_id: RequestId) -> Outbound {
        let mut envelope = remote::invoke(
            device(LOCAL),
            device(PEER),
            &Invoke {
                plugin_id: synra_protocol::ids::PluginId::try_from("@acme/opener".to_owned())
                    .expect("a plugin id"),
                plugin_version: "1.0.0".to_owned(),
                action: "open".to_owned(),
                args: serde_json::json!({"url": "https://example.com"}),
            },
            TraceId::new(),
            0,
        )
        .expect("an invocation");
        envelope.request_id = request_id;
        Outbound {
            to: device(PEER),
            request_id,
            envelope,
        }
    }

    #[test]
    fn the_answer_to_an_invocation_comes_back_on_the_same_link() {
        let (mut here, mut there) = linked();
        let request_id = RequestId::new();
        let outbound = invitation(request_id);

        let far = std::thread::spawn(move || {
            let request = there.receive().expect("the invocation arrives");
            assert_eq!(
                request.request_id, request_id,
                "the id a resend and a reply are keyed on"
            );
            let mut reply = remote::result(
                device(PEER),
                device(LOCAL),
                &InvokeResult::from_execution(Ok("opened".to_owned()), 3),
                request.trace_id,
                1,
            )
            .expect("a result");
            reply.reply_to = Some(request.request_id);
            there.send(&reply).expect("the answer goes back");
        });

        let answer = exchange_on(&mut here, &outbound, Some(Duration::from_secs(1)))
            .expect("the exchange works")
            .expect("and there is an answer");
        far.join().expect("the far side finished");

        assert_eq!(
            answer.event.as_str(),
            synra_protocol::event::events::INTENT_RESULT
        );
        assert_eq!(
            answer.reply_to,
            Some(request_id),
            "the answer names the question"
        );
    }

    #[test]
    fn a_peer_that_does_not_answer_leaves_the_outcome_to_the_listener() {
        // ★ The timeout case: nothing is invented. `None` is "no answer inside the wait", not "it failed": the action may
        // still be running over there, and the listener applies a late result.
        let (mut here, _there) = linked();
        let outbound = invitation(RequestId::new());

        let answer = exchange_on(&mut here, &outbound, Some(Duration::from_millis(20)))
            .expect("a timeout is not an error");
        assert!(answer.is_none(), "no answer was invented");
    }

    #[test]
    fn a_withdrawal_is_sent_and_not_waited_for() {
        let (mut here, mut there) = linked();
        let request_id = RequestId::new();
        let mut envelope = remote::cancel(
            device(LOCAL),
            device(PEER),
            &remote::Cancel {
                reason: "the user changed their mind".to_owned(),
            },
            request_id,
            TraceId::new(),
            0,
        )
        .expect("a withdrawal");
        envelope.request_id = request_id;
        let outbound = Outbound {
            to: device(PEER),
            request_id,
            envelope,
        };

        let far = std::thread::spawn(move || {
            there
                .receive()
                .expect("the withdrawal arrives")
                .event
                .as_str()
                .to_owned()
        });
        let started = std::time::Instant::now();
        let answer =
            exchange_on(&mut here, &outbound, wait_for(&outbound)).expect("the withdrawal is sent");
        let event = far.join().expect("the far side finished");

        assert_eq!(event, synra_protocol::event::events::INTENT_CANCEL);
        assert!(answer.is_none(), "a withdrawal has no answer to wait for");
        // ★ The wait really was skipped. A withdrawal that waited would take the whole eight-second timeout, and the
        // test would hang rather than fail — so the bound is asserted explicitly, with room for a slow machine.
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "a withdrawal does not wait for an answer"
        );
    }

    #[test]
    fn an_unexpected_answer_is_refused_rather_than_read_as_silence() {
        let (mut here, mut there) = linked();
        let outbound = invitation(RequestId::new());

        let far = std::thread::spawn(move || {
            let request = there.receive().expect("the invocation arrives");
            let mut other = remote::result(
                device(PEER),
                device(LOCAL),
                &InvokeResult::from_execution(Ok("opened".to_owned()), 1),
                request.trace_id,
                1,
            )
            .expect("a result");
            other.event = synra_protocol::event::EventName::parse("device.ping").expect("an event");
            other.reply_to = Some(request.request_id);
            there.send(&other).expect("the wrong answer is sent");
        });

        let refused = exchange_on(&mut here, &outbound, Some(Duration::from_secs(1)))
            .expect_err("an event that is not an intent result is a protocol mistake");
        far.join().expect("the far side finished");

        assert_eq!(refused.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn a_device_with_no_address_is_refused_before_a_socket_is_opened() {
        // ★ The refusal the user can act on: "paired but never dialled" is a different fact from "not paired", and the
        // reason says which. The fixture has no paired devices at all, so this is the no-row branch.
        let state = crate::testing::state();
        let outbound = invitation(RequestId::new());

        let refused = deliver_decided(&state, &outbound).expect_err("nothing to dial");
        assert_eq!(refused.code, ErrorCode::Unsupported);
    }

    #[test]
    fn an_invocation_waits_and_a_withdrawal_does_not() {
        // The derivation the caller does not do by hand: the event decides. A cancellation answered by a wait would be
        // eight seconds spent learning that the protocol has no reply to it.
        let invitation = invitation(RequestId::new());
        assert_eq!(
            wait_for(&invitation),
            Some(synra_core::dispatch::REMOTE_RESULT_WAIT)
        );
        let withdrawal = remote::cancel(
            device(LOCAL),
            device(PEER),
            &remote::Cancel {
                reason: "stop".to_owned(),
            },
            RequestId::new(),
            TraceId::new(),
            0,
        )
        .expect("a withdrawal");
        let outbound = Outbound {
            to: device(PEER),
            request_id: RequestId::new(),
            envelope: withdrawal,
        };
        assert!(wait_for(&outbound).is_none());
    }

    #[test]
    fn the_link_carries_the_envelopes_it_sends_into_the_metric_recorder() {
        // The counters live on the link, and an intent envelope is the half a transfer-only recorder would miss. Not a
        // number, because a frame's size is the protocol's: the claim is that the series exists and moved.
        use synra_observe::metrics::Value;

        let (here, mut there) = linked();
        let metrics = Arc::new(synra_observe::metrics::RingRecorder::new());
        let mut link = here.with_metrics(Arc::clone(&metrics));
        let outbound = invitation(RequestId::new());

        let far = std::thread::spawn(move || {
            let request = there.receive().expect("the invocation arrives");
            let mut reply = remote::result(
                device(PEER),
                device(LOCAL),
                &InvokeResult::from_execution(Ok("opened".to_owned()), 1),
                request.trace_id,
                1,
            )
            .expect("a result");
            reply.reply_to = Some(request.request_id);
            there.send(&reply).expect("the answer goes back");
        });

        let _ = exchange_on(&mut link, &outbound, Some(Duration::from_secs(1)))
            .expect("the exchange works");
        far.join().expect("the far side finished");

        match metrics
            .snapshot()
            .get("frames_sent_total{event=_synra.intent.invoke}")
        {
            Some(Value::Counter(count)) => assert_eq!(*count, 1, "the invocation was counted"),
            other => panic!("expected a frame counter for the invocation, got {other:?}"),
        }
    }
}
