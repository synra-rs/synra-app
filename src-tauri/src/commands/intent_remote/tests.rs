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
