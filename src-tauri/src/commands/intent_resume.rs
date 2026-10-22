//! Sending again the dispatches a restart left waiting.
//!
//! # Why this exists
//!
//! Because the board now survives a restart, and that made a new fact visible: a `Dispatching` intent is a request whose
//! answer has not arrived, and the process that sent it is gone. The original send waited [`REMOTE_RESULT_WAIT`] and then
//! left the intent in flight — the honest choice, since the action may still be running over there. But a process that
//! restarts has no second chance unless something takes one.
//!
//! The **request identifier** is what makes taking it safe. It is the receiving side's idempotency key: re-sending the
//! same invocation either runs the action once (the first copy never arrived) or returns the outcome that already
//! happened (it did, and its reply was lost). That is why this re-uses the envelope [`orchestrate::resume`] rebuilds,
//! rather than minting a new one.
//!
//! # What this is, and what it is not
//!
//! One immediate pass at start-up, then the kernel's **bounded retry schedule** ([`RetryPolicy::default`]): what is still in
//! flight is retried eight times, half a minute apart at first and five minutes at the cap, and then stops until the next
//! start. The schedule itself is [`synra_core::intent::retry`], shared with the daemon; this file is the shell's half — the
//! dial, the event, and the row.
//!
//! [`REMOTE_RESULT_WAIT`]: synra_core::dispatch::REMOTE_RESULT_WAIT

use std::collections::HashSet;
use std::time::Duration;

use synra_core::dispatch::orchestrate;
use synra_core::intent::remote::Outbound;
use synra_core::intent::retry::RetryQueue;
use synra_core::supervisor::Cancellation;
use synra_ipc::IpcError;
use synra_protocol::envelope::Envelope;
use synra_protocol::ids::IntentId;

pub use synra_core::intent::retry::RetryPolicy;

use crate::state::AppState;

/// What one pass did.
///
/// Counts rather than a list of failures: the caller writes one line, and the per-intent detail is already in the log
/// beside the send that failed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Resumed {
    /// Dispatches this pass tried to send again.
    pub attempted: usize,
    /// Dispatches whose answer arrived and was applied.
    pub answered: usize,
    /// Dispatches that were sent and answered with nothing inside the wait.
    pub unanswered: usize,
    /// Dispatches whose send could not be made at all.
    pub failed: usize,
}

/// The dispatches a restart should send again, with the envelope each one needs.
///
/// # Why the board is read once
///
/// Because a send blocks for up to the whole wait, and holding the board's lock across it would make every intent command
/// wait for the network. The entries are owned values; the board is released before the first dial.
#[must_use]
pub fn outstanding(state: &AppState) -> Vec<(IntentId, Outbound)> {
    let local = state.identity.lock().device_id;
    let now = synra_store::sqlite::now_ms();
    let board = state.board.lock();
    board
        .all()
        .into_iter()
        .filter_map(|intent| orchestrate::resume(intent, local, now).map(|outbound| (intent.id, outbound)))
        .collect()
}

/// How one attempt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The answer arrived and was applied.
    Answered,
    /// It was sent, and the peer did not answer inside the wait.
    Unanswered,
    /// The send itself could not be made, or the answer could not be applied.
    Failed,
}

/// Sends one intent again, applying whatever answer comes back.
///
/// # Why the closure is a parameter
///
/// Because the send needs a peer and the board does not: a memory link is the transport's test, and this function's subject
/// is what happens to the board when an answer does or does not come back. The loop in [`run`] passes the real send.
fn attempt_with<F>(
    state: &AppState,
    id: IntentId,
    outbound: &Outbound,
    deliver: &mut F,
) -> Outcome
where
    F: FnMut(&Outbound) -> Result<Option<Envelope>, IpcError>,
{
    match deliver(outbound) {
        Ok(Some(reply)) => match orchestrate::apply_result(&state.board, &reply) {
            Ok(Some(applied)) => {
                // The answer changed the intent; both readers learn about it. A reply that applies to some other intent
                // would be a correlation bug elsewhere, and is counted rather than announced.
                if applied == id {
                    crate::commands::intent::announce(state, id);
                    crate::commands::intent::checkpoint(state, id);
                }
                else {
                    eprintln!("synra: an answer to {id} was applied to {applied}");
                }
                Outcome::Answered
            }
            // A reply this device is not waiting on: the intent may have been cancelled while the answer was in flight.
            // Nothing to apply, and nothing to report.
            Ok(None) => Outcome::Unanswered,
            Err(error) => {
                eprintln!("synra: the answer to {id} could not be applied: {error}");
                Outcome::Failed
            }
        },
        Ok(None) => Outcome::Unanswered,
        Err(error) => {
            eprintln!("synra: intent {id} could not be sent again: {error:?}");
            Outcome::Failed
        }
    }
}

/// Sends every outstanding dispatch again, once.
///
/// The immediate pass: what start-up does before the schedule takes over, and what a test can drive without a clock.
pub fn resume_with<F>(state: &AppState, mut deliver: F) -> Resumed
where
    F: FnMut(&Outbound) -> Result<Option<Envelope>, IpcError>,
{
    let mut report = Resumed::default();
    for (id, outbound) in outstanding(state) {
        report.attempted += 1;
        match attempt_with(state, id, &outbound, &mut deliver) {
            Outcome::Answered => report.answered += 1,
            Outcome::Unanswered => report.unanswered += 1,
            Outcome::Failed => report.failed += 1,
        }
    }
    report
}

/// [`resume_with`] over the shell's real transport.
#[must_use]
pub fn resume(state: &AppState) -> Resumed {
    resume_with(state, |outbound| {
        crate::commands::intent_remote::deliver_decided(state, outbound)
    })
}


/// Publishes one intent's retry status and tells the interface when it changed.
///
/// # Why the change check
///
/// Because every retry produces an event only when there is something new to see; a card that re-rendered with the same
/// numbers would be noise on every failed dial.
fn publish(state: &AppState, id: IntentId, status: Option<synra_ipc::IntentRetry>) {
    let changed = {
        let mut retries = state.intent_retries.lock();
        match status {
            Some(status) => retries.insert(id, status) != Some(status),
            None => retries.remove(&id).is_some(),
        }
    };
    if changed {
        crate::commands::intent::announce(state, id);
    }
}

/// The kernel's status in the interface's shape.
///
/// The two types say the same thing with the same field names; the conversion is here rather than in the DTO because the
/// kernel does not depend on the interface crate, and it should not start to for three numbers.
fn retry_status(queue: &RetryQueue, id: IntentId) -> Option<synra_ipc::IntentRetry> {
    queue.status(id).map(|status| synra_ipc::IntentRetry {
        attempts: status.attempts,
        max_attempts: status.max_attempts,
        gave_up: status.gave_up,
        next_at_ms: status.next_at_ms,
    })
}

/// Writes this intent's schedule down, or removes it when the intent is no longer being retried.
fn persist(state: &AppState, queue: &RetryQueue, id: IntentId) {
    let db = state.db();
    let outcome = match queue.row(id) {
        Some(row) => db.save_intent_retry(&row),
        None => db.forget_intent_retry(&id.to_string()),
    };
    drop(db);
    if let Err(error) = outcome {
        // A schedule that did not reach the disk costs one restart's worth of backoff, not the action: the intent is
        // still in flight and the next start sends it again either way.
        eprintln!("synra: the retry schedule for {id} could not be written: {error}");
    }
}

/// Removes a row whose intent is no longer in flight.
fn forget_row(state: &AppState, intent: &str) {
    if let Err(error) = state.db().forget_intent_retry(intent) {
        eprintln!("synra: the stale retry schedule for {intent} could not be removed: {error}");
    }
}

/// The immediate pass, then the bounded retry schedule, until cancelled.
///
/// # Why this is a loop and not a supervisor task
///
/// Because the unit of work is **per-intent** and the supervisor's unit is per-task: eight intents with eight schedules
/// are one background loop, and registering eight tasks would make a shutdown list that changes shape as intents come and
/// go. The loop owns the token and checks it between attempts; a shutdown waits at most one send plus one sleep slice.
///
/// # What it never does
///
/// It never mints a new request id (see [`orchestrate::resume`]), and it never *fails* an intent: a dispatch that was not
/// answered stays `Dispatching` for the 24-hour expiry or a late answer. Giving up on retries is a decision about this
/// process's effort, not about the action's outcome.
pub fn run(state: &AppState, cancel: &Cancellation, policy: RetryPolicy) {
    let mut deliver = |outbound: &Outbound| {
        crate::commands::intent_remote::deliver_decided(state, outbound)
    };

    let first = resume_with(state, &mut deliver);
    if first.attempted > 0 {
        eprintln!(
            "synra: re-sent {} dispatch(es): {} answered, {} unanswered, {} failed",
            first.attempted, first.answered, first.unanswered, first.failed
        );
    }

    // ★ The immediate pass sends everything once; the stored schedule then says how much of the backoff has already been
    // spent. A restart is a new chance, and it is also not a reason to start the wait from thirty seconds again.
    let pending = outstanding_ids(state);
    let rows = match state.db().load_intent_retries() {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("synra: the stored retry schedules could not be read: {error}");
            Vec::new()
        }
    };
    let now_ms = synra_store::sqlite::now_ms;
    let pending_set: HashSet<IntentId> = pending.iter().copied().collect();
    let (mut queue, stale) = RetryQueue::restore(policy, &rows, &pending_set, now_ms());
    for intent in stale {
        forget_row(state, &intent);
    }
    for id in &pending {
        persist(state, &queue, *id);
        publish(state, *id, retry_status(&queue, *id));
    }

    loop {
        if cancel.is_cancelled() {
            return;
        }

        for id in queue.take_due(now_ms()) {
            // The intent may have ended between the snapshot and this moment.
            let Some((_, outbound)) = outstanding(state).into_iter().find(|(candidate, _)| *candidate == id) else {
                queue.forget(id);
                persist(state, &queue, id);
                publish(state, id, None);
                continue;
            };
            match attempt_with(state, id, &outbound, &mut deliver) {
                Outcome::Answered => {
                    queue.forget(id);
                    persist(state, &queue, id);
                    publish(state, id, None);
                }
                Outcome::Unanswered | Outcome::Failed => {
                    let more = queue.record_failure(id, now_ms());
                    persist(state, &queue, id);
                    publish(state, id, retry_status(&queue, id));
                    if !more {
                        eprintln!(
                            "synra: intent {id} was re-sent {} times without an answer; it stays dispatching until it expires",
                            policy.max_attempts
                        );
                    }
                }
            }
        }

        // New dispatches can appear while a send is in flight; the next wake has to see them, and their first schedule
        // has to be written down before the process can end.
        for id in queue.observe(outstanding_ids(state), now_ms()) {
            persist(state, &queue, id);
            publish(state, id, retry_status(&queue, id));
        }
        let wait = queue
            .next_wait(now_ms())
            .min(Duration::from_secs(30))
            .max(Duration::from_millis(10));
        sleep_unless_cancelled(cancel, wait);
    }
}

/// The ids the loop would send right now.
fn outstanding_ids(state: &AppState) -> Vec<IntentId> {
    outstanding(state).into_iter().map(|(id, _)| id).collect()
}

/// Sleeps in slices, so a shutdown does not wait out a five-minute backoff.
fn sleep_unless_cancelled(cancel: &Cancellation, wait: Duration) {
    let mut left = wait;
    while !left.is_zero() {
        if cancel.is_cancelled() {
            return;
        }
        let slice = left.min(Duration::from_millis(250));
        std::thread::sleep(slice);
        left = left.saturating_sub(slice);
    }
}

#[cfg(test)]
mod tests {
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
            &InvokeResult {
                ok: true,
                result: "opened".to_owned(),
                error: None,
                error_code: None,
                elapsed_ms: 2,
            },
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
            assert_eq!(outbound.request_id, request, "the idempotency key is the stored one");
            assert_eq!(outbound.to, peer);
            Ok(Some(reply.clone()))
        });

        assert_eq!(report, Resumed { attempted: 1, answered: 1, unanswered: 0, failed: 0 });
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
        assert_eq!(row.state, "succeeded", "the next start reads the outcome, not another dispatch");
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

        assert_eq!(report, Resumed { attempted: 1, answered: 0, unanswered: 0, failed: 1 });
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

        assert_eq!(report, Resumed { attempted: 1, answered: 0, unanswered: 1, failed: 0 });
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
        assert!(outstanding(&state).iter().all(|(outstanding, _)| *outstanding != id));
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
        assert_eq!(crate::commands::intent::intent_snapshot(&state, id).expect("a card").retry, None);
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
        handle.join().expect("the loop returned after the cancellation");
    }
}
