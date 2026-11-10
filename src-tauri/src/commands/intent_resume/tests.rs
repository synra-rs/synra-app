
use synra_core::intent::remote::{self, InvokeResult};
use synra_core::intent::{Choice, IntentSource};
use synra_protocol::ids::{DeviceId, PluginId, RequestId, TraceId};

use super::*;

fn direct(target: DeviceId) -> IntentSource {
    IntentSource::Direct {
        plugin_id: PluginId::parse("@acme/opener").expect("a plugin id"),
        action: "open".to_owned(),
        params: serde_json::json!({"url": "https://example.com"}),
        target,
    }
}

/// Raises the dispatch a restart would have found, and answers what it needs to be answered by.
fn dispatched(state: &AppState) -> (IntentId, TraceId, RequestId, DeviceId, DeviceId) {
    let local = state.identity.lock().device_id;
    let peer = DeviceId::new();
    let request = RequestId::new();
    let (id, trace, created) = state.board.lock().raise(
        direct(peer),
        Choice::Automatic,
        None,
        Some(request),
        synra_store::sqlite::now_ms(),
    );
    assert!(created, "a direct source is raised rather than matched");
    (id, trace, request, peer, local)
}

fn answer(from: DeviceId, to: DeviceId, trace: TraceId, request: RequestId) -> Envelope {
    let mut reply = remote::result(
        from,
        to,
        &InvokeResult::from_execution(Ok("opened".to_owned()), 2),
        trace,
        2_000,
    )
    .expect("a result");
    reply.reply_to = Some(request);
    reply
}

#[test]
fn a_reply_to_the_resend_ends_the_intent_and_is_written_down() {
    // ★ The success path, and the half that makes a restart worth doing: the answer arrives, the card changes, and
    // the row the next start reads is the finished one rather than another "dispatching".
    let state = crate::testing::state();
    let (id, trace, request, peer, local) = dispatched(&state);
    let reply = answer(peer, local, trace, request);

    let report = resume_with(&state, |outbound| {
        assert_eq!(
            outbound.request_id, request,
            "the idempotency key is the stored one"
        );
        assert_eq!(outbound.to, peer);
        Ok(Some(reply.clone()))
    });

    assert_eq!(
        report,
        Resumed {
            attempted: 1,
            answered: 1,
            unanswered: 0,
            failed: 0
        }
    );
    let card = crate::commands::intent::intent_list(&state)
        .expect("the board answers")
        .into_iter()
        .find(|intent| intent.intent_id == id)
        .expect("the intent is there");
    assert_eq!(card.state, "succeeded");
    let row = state
        .db()
        .load_intents()
        .expect("the rows are readable")
        .into_iter()
        .find(|row| row.intent_id == id.to_string())
        .expect("the intent was written down");
    assert_eq!(
        row.state, "succeeded",
        "the next start reads the outcome, not another dispatch"
    );
}

#[test]
fn a_send_that_fails_leaves_the_dispatch_alone() {
    // ★ A failed send is not an outcome: the peer may have run the action, or not. The intent stays in flight so a
    // later start can try again and a late answer can still be applied.
    let state = crate::testing::state();
    let (id, ..) = dispatched(&state);

    let report = resume_with(&state, |_| {
        Err(IpcError::unknown(synra_protocol::ErrorCode::DeviceOffline))
    });

    assert_eq!(
        report,
        Resumed {
            attempted: 1,
            answered: 0,
            unanswered: 0,
            failed: 1
        }
    );
    let card = crate::commands::intent::intent_list(&state)
        .expect("the board answers")
        .into_iter()
        .find(|intent| intent.intent_id == id)
        .expect("the intent is still there");
    assert_eq!(card.state, "dispatching");
}

#[test]
fn a_peer_that_does_not_answer_counts_as_unanswered_rather_than_failed() {
    let state = crate::testing::state();
    let _ = dispatched(&state);

    let report = resume_with(&state, |_| Ok(None));

    assert_eq!(
        report,
        Resumed {
            attempted: 1,
            answered: 0,
            unanswered: 1,
            failed: 0
        }
    );
}

#[test]
fn work_that_never_left_is_not_in_the_outstanding_set() {
    // The filter is the core's; this is the shell's claim that it uses it. A peer's request is running **here**, and
    // re-sending it would ask the peer to run it again.
    let state = crate::testing::state();
    let peer = DeviceId::new();
    let (id, _, _) = state.board.lock().raise(
        direct(peer),
        Choice::Automatic,
        Some(peer),
        Some(RequestId::new()),
        synra_store::sqlite::now_ms(),
    );
    assert!(
        outstanding(&state)
            .iter()
            .all(|(outstanding, _)| *outstanding != id)
    );
}

#[test]
fn the_published_status_reaches_the_snapshot_and_clears_when_the_intent_ends() {
    // ★ The page reads the same `intent_snapshot` it always did; the retry count travels in that answer rather than
    // in a second channel, so the two cannot disagree about how many times this device has tried.
    let state = crate::testing::state();
    let (id, ..) = dispatched(&state);
    let retry = synra_ipc::IntentRetry {
        attempts: 2,
        max_attempts: 8,
        gave_up: false,
        next_at_ms: Some(99_000),
    };
    publish(&state, id, Some(retry));

    let card = crate::commands::intent::intent_snapshot(&state, id).expect("a card");
    assert_eq!(card.retry, Some(retry));

    publish(&state, id, None);
    assert_eq!(
        crate::commands::intent::intent_snapshot(&state, id)
            .expect("a card")
            .retry,
        None
    );
}
#[test]
fn the_loop_stops_when_it_is_cancelled() {
    // ★ The shutdown path: the loop sleeps between attempts and up to thirty seconds when nothing is pending, so it has
    // to notice a cancellation inside a slice rather than after the whole wait. A thread left sleeping through a quit
    // is a process that will not close.
    let state = crate::testing::state();
    let cancel = Cancellation::new();
    let token = cancel.clone();
    let handle = std::thread::spawn(move || run(&state, &token, RetryPolicy::default()));

    std::thread::sleep(Duration::from_millis(30));
    cancel.cancel();
    handle
        .join()
        .expect("the loop returned after the cancellation");
}
