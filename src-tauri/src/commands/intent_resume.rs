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
//! One immediate pass at start-up, then a **bounded retry schedule**: [`RetryPolicy::default`] retries what is still in
//! flight eight times, half a minute apart at first and five minutes at the cap, and then stops until the next start.
//! It is **not** a queue: attempts are not persisted, there is no ordering, and a process that ends loses the schedule.
//! "Offline queue" would be a claim about durability this does not have.
//!
//! [`REMOTE_RESULT_WAIT`]: synra_core::dispatch::REMOTE_RESULT_WAIT

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use synra_core::dispatch::orchestrate;
use synra_core::intent::remote::Outbound;
use synra_core::supervisor::{Cancellation, RestartPolicy, TaskId};
use synra_ipc::IpcError;
use synra_protocol::envelope::Envelope;
use synra_protocol::ids::IntentId;

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

/// How often a dispatch that is still in flight is sent again.
///
/// # Why these numbers
///
/// A dispatch that timed out has just spent [`synra_core::dispatch::REMOTE_RESULT_WAIT`] waiting; retrying immediately
/// would be a busy loop against a device that is asleep. Half a minute is long enough for a peer to come back from a
/// suspend and short enough to be useful; the cap is five minutes because a device that is off will be off for a while,
/// and the attempt bound (eight) is what keeps a day's worth of retries from becoming a day's worth of dials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// The wait before the first retry.
    pub initial: Duration,
    /// The longest wait between retries.
    pub max: Duration,
    /// How many retries before this pass gives up until the next start.
    pub max_attempts: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(30),
            max: Duration::from_secs(300),
            max_attempts: 8,
        }
    }
}

impl RetryPolicy {
    /// The wait before retry number `attempt`, jittered by the intent's own name.
    ///
    /// ★ Reusing the supervisor's policy rather than writing a second backoff: the shape (double, cap, add a
    /// name-derived jitter so two intents do not retry in lockstep) is already implemented and already tested.
    fn delay(self, id: IntentId, attempt: u32) -> Duration {
        RestartPolicy::Backoff { max_attempts: self.max_attempts, initial: self.initial, max: self.max }
            .delay_for(attempt, &TaskId::new(id.to_string()))
    }

    /// The same wait in the store's clock, saturating rather than wrapping.
    fn delay_ms(self, id: IntentId, attempt: u32) -> i64 {
        i64::try_from(self.delay(id, attempt).as_millis()).unwrap_or(i64::MAX)
    }
}

/// When each outstanding dispatch is next due, and how many times it has been retried.
///
/// # Why the clock is milliseconds rather than an `Instant`
///
/// Because the schedule has to survive a restart, and an `Instant` is meaningless to the next process. The queue speaks the
/// same wall clock the store writes; a test moves a number forward instead of sleeping for five minutes.
#[derive(Debug)]
struct RetryQueue {
    policy: RetryPolicy,
    /// How many retries have been scheduled, counting the next one. Missing means "not seen yet".
    attempts: HashMap<IntentId, u32>,
    /// When the next attempt is due, in milliseconds since the epoch. Missing means "not waiting".
    due: HashMap<IntentId, i64>,
    /// Intents this pass stopped retrying. Kept rather than forgotten because the interface is told "no answer yet" —
    /// the intent itself is still `Dispatching`, and the entry goes when the intent ends.
    gave_up: HashSet<IntentId>,
}

impl RetryQueue {
    fn new(policy: RetryPolicy) -> Self {
        Self { policy, attempts: HashMap::new(), due: HashMap::new(), gave_up: HashSet::new() }
    }

    /// Rebuilds the schedule from stored rows.
    ///
    /// # What is restored, and what is deliberately not
    ///
    /// The attempt count and the next due time are restored, so a restart does not send the backoff back to thirty seconds.
    /// A **given-up** row is not restored as given up: "gave up until the next start" is what the interface says, and the
    /// next start is this one. Such a row is returned in the second half of the answer for the caller to delete, and the
    /// intent gets a fresh first retry.
    ///
    /// Rows whose intent is no longer pending are returned the same way — a schedule for work that ended is not something
    /// to keep.
    fn restore(
        policy: RetryPolicy,
        rows: &[synra_store::IntentRetryRow],
        pending: &HashSet<IntentId>,
        now_ms: i64,
    ) -> (Self, Vec<String>) {
        let mut queue = Self::new(policy);
        let mut stale = Vec::new();
        let stored: HashMap<&str, &synra_store::IntentRetryRow> =
            rows.iter().map(|row| (row.intent_id.as_str(), row)).collect();

        for id in pending {
            let key = id.to_string();
            match stored.get(key.as_str()) {
                Some(row) if !row.gave_up => {
                    queue.attempts.insert(*id, row.attempts.saturating_add(1));
                    // A stored time in the past is due now; the loop's `take_due` treats it that way without a special case.
                    queue.due.insert(*id, row.next_at_ms.unwrap_or_else(|| now_ms + queue.policy.delay_ms(*id, 1)));
                }
                Some(row) => {
                    stale.push(row.intent_id.clone());
                    queue.attempts.insert(*id, 1);
                    queue.due.insert(*id, now_ms + queue.policy.delay_ms(*id, 1));
                }
                None => {
                    queue.attempts.insert(*id, 1);
                    queue.due.insert(*id, now_ms + queue.policy.delay_ms(*id, 1));
                }
            }
        }
        for row in rows {
            if !pending.iter().any(|id| id.to_string() == row.intent_id) {
                stale.push(row.intent_id.clone());
            }
        }
        (queue, stale)
    }

    /// Schedules anything new, and forgets anything that is no longer in flight.
    ///
    /// # Why the sweep is part of observing
    ///
    /// Because an intent can end for reasons this loop did not cause — the user cancelled it, the answer arrived on the
    /// listener's link, or the retention sweep dropped it. Keeping its entry would mean a due time for work that no
    /// longer exists, and the map would grow for the life of the process.
    ///
    /// # Returns
    ///
    /// The intents that had no entry, so the caller knows which rows it has to write.
    fn observe(&mut self, pending: impl IntoIterator<Item = IntentId>, now_ms: i64) -> Vec<IntentId> {
        let pending: HashSet<IntentId> = pending.into_iter().collect();
        self.attempts.retain(|id, _| pending.contains(id));
        self.due.retain(|id, _| pending.contains(id));
        self.gave_up.retain(|id| pending.contains(id));
        let mut added = Vec::new();
        for id in pending {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.attempts.entry(id) {
                entry.insert(1);
                self.due.insert(id, now_ms + self.policy.delay_ms(id, 1));
                added.push(id);
            }
        }
        added
    }

    /// The ids whose wait is over, removed from the schedule.
    ///
    /// Removed rather than left: the caller is about to attempt them, and a due time that is still in the past would make
    /// the next `take_due` hand the same id out again.
    fn take_due(&mut self, now_ms: i64) -> Vec<IntentId> {
        let due: Vec<IntentId> = self
            .due
            .iter()
            .filter_map(|(id, at)| (*at <= now_ms).then_some(*id))
            .collect();
        for id in &due {
            self.due.remove(id);
        }
        due
    }

    /// Schedules the next attempt, or gives up when the bound is reached.
    ///
    /// Returns whether another attempt was scheduled. Giving up keeps the entry and its count: the card says "no answer
    /// after N tries", and dropping the entry at the same moment would leave the page with nothing to say.
    fn record_failure(&mut self, id: IntentId, now_ms: i64) -> bool {
        let attempt = self.attempts.entry(id).or_insert(0);
        *attempt = attempt.saturating_add(1);
        if *attempt > self.policy.max_attempts {
            self.gave_up.insert(id);
            return false;
        }
        self.due.insert(id, now_ms + self.policy.delay_ms(id, *attempt));
        true
    }

    /// Removes an intent: it was answered, or it ended.
    fn forget(&mut self, id: IntentId) {
        self.attempts.remove(&id);
        self.due.remove(&id);
        self.gave_up.remove(&id);
    }

    /// What the interface is told about one intent's retries, when there is anything to tell.
    ///
    /// Nothing before the first retry has failed: an intent whose first attempt is still waiting is simply `Dispatching`,
    /// and "retrying (0/8)" would describe work that has not happened yet.
    fn status(&self, id: IntentId) -> Option<synra_ipc::IntentRetry> {
        let next = *self.attempts.get(&id)?;
        let attempts = next.saturating_sub(1);
        let gave_up = self.gave_up.contains(&id);
        if attempts == 0 && !gave_up {
            return None;
        }
        Some(synra_ipc::IntentRetry {
            attempts,
            max_attempts: self.policy.max_attempts,
            gave_up,
        })
    }

    /// The row that preserves this intent's schedule, when it has one.
    ///
    /// `attempts` is the count of retries **already made**, which is one less than the internal next-attempt number.
    fn row(&self, id: IntentId) -> Option<synra_store::IntentRetryRow> {
        let next = *self.attempts.get(&id)?;
        Some(synra_store::IntentRetryRow {
            intent_id: id.to_string(),
            attempts: next.saturating_sub(1),
            next_at_ms: self.due.get(&id).copied(),
            gave_up: self.gave_up.contains(&id),
        })
    }

    /// How long until the next attempt, or [`Duration::MAX`] when nothing is waiting.
    fn next_wait(&self, now_ms: i64) -> Duration {
        self.due
            .values()
            .map(|at| at.saturating_sub(now_ms).max(0))
            .filter_map(|ms| u64::try_from(ms).ok())
            .min()
            .map(Duration::from_millis)
            .unwrap_or(Duration::MAX)
    }
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
        publish(state, *id, queue.status(*id));
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
                    publish(state, id, queue.status(id));
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
            publish(state, id, queue.status(id));
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

    // ── the schedule ──────────────────────────────────────────────────────────────────────────────

    #[test]
    fn a_pending_dispatch_is_not_due_until_its_first_wait_has_passed() {
        // ★ The immediate pass has just run; retrying at zero would be a busy loop against a device that is asleep.
        let mut queue = RetryQueue::new(RetryPolicy::default());
        let id = IntentId::new();
        let now = 1_000;
        let _ = queue.observe([id], now);

        assert!(queue.take_due(now).is_empty(), "the first wait has not passed");
        assert_eq!(
            queue.take_due(now + 60_000),
            vec![id],
            "a minute is past any first wait of thirty seconds"
        );
    }

    #[test]
    fn each_failed_retry_waits_at_least_as_long_as_the_one_before() {
        // The growth the policy promises. The jitter is added rather than symmetric, so "at least as long" is the real
        // property and a test can assert it without knowing the name-derived offset.
        let policy = RetryPolicy::default();
        let id = IntentId::new();
        let mut previous = Duration::ZERO;
        for attempt in 1..=5 {
            let wait = policy.delay(id, attempt);
            assert!(wait >= previous, "attempt {attempt} waited less: {previous:?} -> {wait:?}");
            assert!(wait <= policy.max, "attempt {attempt} passed the ceiling: {wait:?}");
            previous = wait;
        }
    }

    #[test]
    fn a_retry_that_keeps_failing_is_given_up_on_after_the_bound() {
        // ★ Giving up is about this process's effort, not the action's outcome: the intent stays dispatching, so a late
        // answer can still end it and the 24-hour expiry still collects it.
        let policy = RetryPolicy {
            initial: Duration::from_millis(1),
            max: Duration::from_millis(2),
            max_attempts: 3,
        };
        let mut queue = RetryQueue::new(policy);
        let id = IntentId::new();
        let now = 1_000;
        let _ = queue.observe([id], now);
        let later = now + 10_000;

        for retry in 1..=3 {
            let at = later + i64::from(retry) * 10_000;
            assert_eq!(queue.take_due(at), vec![id], "retry {retry} should be due");
            let more = queue.record_failure(id, at);
            assert_eq!(more, retry < 3, "after retry {retry} another was scheduled: {more}");
        }
        assert!(queue.take_due(later + 60_000).is_empty());
        assert!(queue.gave_up.contains(&id), "the entry is kept so the page can say why");
        assert_eq!(
            queue.status(id),
            Some(synra_ipc::IntentRetry { attempts: 3, max_attempts: 3, gave_up: true })
        );
    }

    #[test]
    fn the_status_is_silent_until_a_retry_has_actually_failed() {
        // ★ An intent whose first attempt is still waiting is `Dispatching`; "retrying (0/8)" would describe work that
        // has not happened.
        let mut queue = RetryQueue::new(RetryPolicy::default());
        let id = IntentId::new();
        let now = 1_000;
        let _ = queue.observe([id], now);
        assert_eq!(queue.status(id), None);

        let at = now + 60_000;
        let _ = queue.take_due(at);
        queue.record_failure(id, at);
        assert_eq!(
            queue.status(id),
            Some(synra_ipc::IntentRetry { attempts: 1, max_attempts: 8, gave_up: false })
        );

        queue.forget(id);
        assert_eq!(queue.status(id), None);
    }

    #[test]
    fn the_published_status_reaches_the_snapshot_and_clears_when_the_intent_ends() {
        // ★ The page reads the same `intent_snapshot` it always did; the retry count travels in that answer rather than
        // in a second channel, so the two cannot disagree about how many times this device has tried.
        let state = crate::testing::state();
        let (id, ..) = dispatched(&state);
        let retry = synra_ipc::IntentRetry { attempts: 2, max_attempts: 8, gave_up: false };
        publish(&state, id, Some(retry));

        let card = crate::commands::intent::intent_snapshot(&state, id).expect("a card");
        assert_eq!(card.retry, Some(retry));

        publish(&state, id, None);
        assert_eq!(crate::commands::intent::intent_snapshot(&state, id).expect("a card").retry, None);
    }

    #[test]
    fn an_intent_that_ended_leaves_the_schedule() {
        // The board is the authority: a cancel, a late answer, or the retention sweep can end an intent while the loop is
        // sleeping, and a due time for work that no longer exists would keep the loop awake and the map growing.
        let mut queue = RetryQueue::new(RetryPolicy::default());
        let id = IntentId::new();
        let _ = queue.observe([id], 1_000);
        let _ = queue.observe(std::iter::empty(), 2_000);

        assert!(queue.attempts.is_empty());
        assert!(queue.take_due(600_000).is_empty());
    }

    #[test]
    fn an_answer_removes_the_schedule_without_a_retry() {
        let mut queue = RetryQueue::new(RetryPolicy::default());
        let id = IntentId::new();
        let _ = queue.observe([id], 1_000);
        queue.forget(id);

        assert!(queue.take_due(600_000).is_empty());
        assert_eq!(queue.next_wait(1_000), Duration::MAX);
    }

    #[test]
    fn a_restart_continues_the_schedule_and_forgets_what_cannot_be_used() {
        // ★ The point of writing the schedule down: a dispatch already retried three times does not begin again at the
        // first wait. A given-up row is deliberately *not* restored as given up — the interface said "until the next
        // start", and this is it — so it is returned as stale and gets a fresh first retry.
        let id = IntentId::new();
        let given_up = IntentId::new();
        let finished = IntentId::new();
        let rows = vec![
            synra_store::IntentRetryRow {
                intent_id: id.to_string(),
                attempts: 3,
                next_at_ms: Some(50_000),
                gave_up: false,
            },
            synra_store::IntentRetryRow {
                intent_id: given_up.to_string(),
                attempts: 8,
                next_at_ms: None,
                gave_up: true,
            },
            synra_store::IntentRetryRow {
                intent_id: finished.to_string(),
                attempts: 2,
                next_at_ms: Some(50_000),
                gave_up: false,
            },
        ];
        let pending: HashSet<IntentId> = [id, given_up].into_iter().collect();

        let (mut queue, stale) = RetryQueue::restore(RetryPolicy::default(), &rows, &pending, 10_000);

        assert_eq!(
            queue.status(id),
            Some(synra_ipc::IntentRetry { attempts: 3, max_attempts: 8, gave_up: false }),
            "the count came back"
        );
        assert!(
            !queue.take_due(49_999).contains(&id),
            "the stored wait is respected: the intent is not due before its stored time"
        );
        assert_eq!(queue.take_due(50_000), vec![id], "and then it is due");

        assert_eq!(queue.status(given_up), None, "a new process gets a fresh chance");
        assert!(stale.contains(&given_up.to_string()), "the given-up row was replaced");
        assert!(stale.contains(&finished.to_string()), "a schedule for work that ended is stale");
        assert_eq!(queue.attempts.get(&given_up), Some(&1), "the fresh first retry was scheduled");
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
