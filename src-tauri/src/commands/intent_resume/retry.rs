use super::*;

/// Runs the immediate resume pass and the bounded retry schedule until cancelled.
pub fn run(state: &AppState, cancel: &Cancellation, policy: RetryPolicy) {
    let mut deliver =
        |outbound: &Outbound| crate::commands::intent_remote::deliver_decided(state, outbound);

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
        persist_row(state, queue.row(*id), *id);
        publish(state, *id, retry_status(&queue, *id));
    }

    let mut host = ShellRetry { state };
    let mut now = now_ms;
    let mut sleep = |wait| sleep_unless_cancelled(cancel, wait);
    run_loop(&mut queue, cancel, &mut host, &mut now, &mut sleep);
}

/// The shell's half of the retry loop.
struct ShellRetry<'a> {
    state: &'a AppState,
}

impl RetryHost for ShellRetry<'_> {
    fn pending(&mut self) -> Vec<IntentId> {
        outstanding_ids(self.state)
    }

    fn attempt(&mut self, id: IntentId) -> Attempt {
        // The intent may have ended between the snapshot and this moment.
        let Some((_, outbound)) = outstanding(self.state)
            .into_iter()
            .find(|(candidate, _)| *candidate == id)
        else {
            return Attempt::Ended;
        };
        let mut deliver = |outbound: &Outbound| {
            crate::commands::intent_remote::deliver_decided(self.state, outbound)
        };
        match attempt_with(self.state, id, &outbound, &mut deliver) {
            Outcome::Answered => Attempt::Answered,
            Outcome::Unanswered => Attempt::Unanswered,
            Outcome::Failed => Attempt::Failed,
        }
    }

    fn changed(
        &mut self,
        id: IntentId,
        row: Option<synra_store::IntentRetryRow>,
        status: Option<RetryStatus>,
    ) {
        persist_row(self.state, row, id);
        publish(self.state, id, status.map(status_ipc));
        if let Some(status) = status
            && status.gave_up
        {
            eprintln!(
                "synra: intent {id} was re-sent {} times without an answer; it stays dispatching until it expires",
                status.max_attempts
            );
        }
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
