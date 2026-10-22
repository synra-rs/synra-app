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
//! # What this is not
//!
//! Not an offline queue: one pass at start-up, not a retry loop. A device that is still unreachable leaves its intent
//! dispatching until the next start or until [`expire_unanswered`](synra_core::intent::persist::expire_unanswered) ends
//! it. The periodic version would need a schedule and a backoff, and calling this "the queue" before it has either would
//! be the same kind of claim the design keeps warning about.
//!
//! [`REMOTE_RESULT_WAIT`]: synra_core::dispatch::REMOTE_RESULT_WAIT

use synra_core::dispatch::orchestrate;
use synra_core::intent::remote::Outbound;
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

/// Sends every outstanding dispatch again, once.
///
/// The closure is the send itself, which is what lets the loop be tested without a peer: a memory link is the transport's
/// test, and this function's subject is what happens to the board when an answer does or does not come back.
pub fn resume_with<F>(state: &AppState, mut deliver: F) -> Resumed
where
    F: FnMut(&Outbound) -> Result<Option<Envelope>, IpcError>,
{
    let mut report = Resumed::default();
    for (id, outbound) in outstanding(state) {
        report.attempted += 1;
        match deliver(&outbound) {
            Ok(Some(reply)) => match orchestrate::apply_result(&state.board, &reply) {
                Ok(Some(applied)) => {
                    // The answer changed the intent; both readers learn about it. A reply that applies to some other
                    // intent would be a correlation bug elsewhere, and is counted rather than announced.
                    if applied == id {
                        crate::commands::intent::announce(state, id);
                        crate::commands::intent::checkpoint(state, id);
                    }
                    else {
                        eprintln!("synra: an answer to {id} was applied to {applied}");
                    }
                    report.answered += 1;
                }
                // A reply this device is not waiting on: the intent may have been cancelled while the answer was in
                // flight. Nothing to apply, and nothing to report.
                Ok(None) => report.unanswered += 1,
                Err(error) => {
                    eprintln!("synra: the answer to {id} could not be applied: {error}");
                    report.failed += 1;
                }
            },
            Ok(None) => report.unanswered += 1,
            Err(error) => {
                eprintln!("synra: intent {id} could not be sent again: {error:?}");
                report.failed += 1;
            }
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
}
