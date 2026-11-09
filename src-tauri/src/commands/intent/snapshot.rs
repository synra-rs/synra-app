use super::{AppState, IntentId, IntentSnapshot, IpcError, as_ipc, candidate_card, orchestrate};

/// Every intent the board is holding, in the order they were raised.
///
/// # What "every" means
///
/// [`Board::all`], not the live ones — the kernel's own reason for `all` existing is that persisting has to save what
/// *changed*, which includes an intent that has just ended. So a finished intent stays in the list with its state saying
/// so, and a panel that showed only what is still running would empty itself at the moment the user wanted to see what
/// happened.
///
/// # Errors
///
/// [`IpcError`] when the board cannot answer for one of the intents it lists. Not reachable through this function — the
/// identifiers come from the board itself — and propagated rather than skipped, because a list that silently dropped an
/// entry renders as "that intent never existed".
#[allow(clippy::result_large_err)]
pub fn intent_list(state: &AppState) -> Result<Vec<IntentSnapshot>, IpcError> {
    // ★ The identifiers are collected and the lock released **before** any answer is built.
    //
    // `snapshot_of` locks the board again, and `parking_lot::Mutex` is not reentrant: holding this guard across the loop
    // would deadlock on the first intent, which is a hang rather than a failure anyone can read.
    let ids: Vec<IntentId> = {
        let board = state.board.lock();
        board.all().iter().map(|intent| intent.id).collect()
    };

    ids.into_iter().map(|id| snapshot_of(state, id)).collect()
}

/// What has become of an intent.
///
/// # Errors
///
/// [`IpcError`] when the identifier is not one, or names an intent this shell never raised.
pub fn intent_snapshot(state: &AppState, intent_id: IntentId) -> Result<IntentSnapshot, IpcError> {
    snapshot_of(state, intent_id)
}

/// Announces an intent's current state, if the board can still answer for it.
///
/// Silent when it cannot: the board losing an intent between the change and the announcement is a race the caller cannot
/// repair, and publishing an error is not what an event is for. The interface's next `intent_list` is the truth either
/// way, so a missed announcement costs one stale list rather than a wrong one.
pub(crate) fn announce(state: &AppState, intent_id: IntentId) {
    if let Ok(snapshot) = snapshot_of(state, intent_id) {
        state.publish(synra_ipc::event::KernelEvent::IntentUpdated(Box::new(
            snapshot,
        )));
    }
}

/// Writes one intent down, as the board now holds it.
///
/// # Why a failure is logged rather than returned
///
/// Because every caller reaches this **after** the change has happened. A command that reported "no" after the action ran
/// would tell the user nothing occurred when something did; the honest pair is the answer plus a line saying the record
/// did not reach the disk. The row mapping itself lives in the kernel because the daemon writes the same rows.
pub(crate) fn checkpoint(state: &AppState, intent_id: IntentId) {
    // ★ The board guard is released before the database is touched, for the reason the daemon documents: the board is
    // the lock every intent command takes, and a write held under it would make the next raise wait for the disk.
    let record = {
        let board = state.board.lock();
        synra_core::intent::persist::record_of(&board, intent_id, synra_store::sqlite::now_ms())
    };
    // Something the retention sweep dropped between the change and the write. There is nothing to record, and inventing
    // a row would make the store hold an intent no board ever had.
    let Some(record) = record else {
        return;
    };
    if record.document.is_empty() {
        eprintln!(
            "synra: intent {intent_id} could not be serialised, so its row carries no document"
        );
    }
    if let Err(error) = state.db().save_intent(&record) {
        eprintln!("synra: intent {intent_id} was not written down: {error}");
    }
}

/// One intent, as the interface is shown it.
///
/// ★ Both reads go through here, and that is the point of it being a function: `intent_list` and `intent_snapshot` answer
/// about the same thing, so a field added to the DTO cannot appear in one answer and be missing from the other.
///
/// # Errors
///
/// [`IpcError`] when the identifier names an intent this shell never raised.
#[allow(clippy::result_large_err)]
pub(super) fn snapshot_of(
    state: &AppState,
    intent_id: IntentId,
) -> Result<IntentSnapshot, IpcError> {
    let view = orchestrate::snapshot(&state.board, &intent_id.to_string())
        .map_err(|error| as_ipc(&error))?;
    Ok(IntentSnapshot {
        intent_id: view.intent_id,
        // ★ The one identifier a user can hand to `synra logs --trace`. The board carried it; the snapshot used to drop
        // it, which made the filter a feature with no way to know what to filter by.
        trace_id: view.trace,
        state: view.state.to_owned(),
        // The kernel's chosen plugin is the one and only candidate once it has settled on one; before that there is no
        // choice to report. Derived rather than sent, because the board does not carry a separate "chosen" field.
        plugin_id: match view.candidates.as_slice() {
            [only] => Some(only.plugin_id.clone()),
            _ => None,
        },
        candidates: view
            .candidates
            .iter()
            .map(|candidate| candidate_card(state, candidate))
            .collect(),
        // ★ No prompt, and that is a real absence rather than an oversight: the kernel asks "which plugin?" and "where?"
        // through its *state*, and the interface's `IntentPrompt` wants a message key and a list of options. Mapping one
        // to the other would be the shell writing the question, which is the kernel's to write.
        prompt: None,
        // ★ The kernel's sentence is not what a user reads: `from_synra_error` keeps the code, turns it into a message key,
        // and carries the sentence as a detail for a bug report. Until this line existed the field was always `None`, so the
        // page's "why did it fail?" line had nothing to show for a real failure.
        error: view.error.as_ref().map(IpcError::from_synra_error),
        // ★ Read from the retry loop's shared map: the loop knows how many times it has sent this again, and the page
        // cannot tell "the first attempt is still running" from "we have retried four times" by the state alone.
        retry: state.intent_retries.lock().get(&intent_id).copied(),
        // ★ The moment it was raised, which the board has always carried. This used to be the moment the snapshot was
        // read: a field named "started" that answered "just now" for an intent that had been running for an hour.
        started_at_ms: view.created_at_ms,
    })
}
