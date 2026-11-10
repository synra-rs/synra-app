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
use synra_core::intent::retry::{Attempt, RetryHost, RetryQueue, RetryStatus, run_loop};
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
        .filter_map(|intent| {
            orchestrate::resume(intent, local, now).map(|outbound| (intent.id, outbound))
        })
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
fn attempt_with<F>(state: &AppState, id: IntentId, outbound: &Outbound, deliver: &mut F) -> Outcome
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
                } else {
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
    queue.status(id).map(status_ipc)
}

/// The same status in the interface's shape.
fn status_ipc(status: RetryStatus) -> synra_ipc::IntentRetry {
    synra_ipc::IntentRetry {
        attempts: status.attempts,
        max_attempts: status.max_attempts,
        gave_up: status.gave_up,
        next_at_ms: status.next_at_ms,
    }
}

/// Writes a schedule down, or removes it when the intent is no longer being retried.
fn persist_row(state: &AppState, row: Option<synra_store::IntentRetryRow>, id: IntentId) {
    let db = state.db();
    let outcome = match row {
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
mod retry;
pub use retry::run;

#[cfg(test)]
mod tests;
