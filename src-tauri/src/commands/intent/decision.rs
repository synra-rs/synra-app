use super::{
    AppState, IntentDecisionDto, IntentId, IpcError, announce, as_ipc, checkpoint, orchestrate,
    plugins,
};

/// Applies the user's answer, and runs what they chose.
///
/// # Where the action happens
///
/// A target that is this device runs here, through the orchestration. A target that is another device becomes an
/// [`orchestrate::Outbound`] and is **sent**, not run: doing it here would perform the user's action on the machine they
/// shared from rather than the one they picked. The exchange is [`crate::commands::intent_remote::deliver_decided`], and
/// the answer it reads is applied to the board before the card is announced again.
///
/// # Why the two announcements
///
/// The first one says what the decision did (dispatching, or cancelled for a decline); the second says what the far side
/// answered. Without the first, a remote action would leave the card unchanged for up to
/// [`synra_core::dispatch::REMOTE_RESULT_WAIT`] — a spinner with no state, on the one path where the user is most likely
/// to wonder whether their click registered.
///
/// # Errors
///
/// [`IpcError`] when the intent is unknown or is not in a state that can be decided, when the envelope cannot be
/// delivered (no address, no route, a refused write), or when the answer cannot be applied. A decision that landed is
/// **not** undone by a failed delivery: the board keeps the state the decision produced, and the error says what the
/// other device was not told.
pub fn intent_decide(
    state: &AppState,
    intent_id: IntentId,
    decision: &IntentDecisionDto,
) -> Result<(), IpcError> {
    let plugins = plugins(state)?;
    let local = state.identity.lock().device_id;
    let id = intent_id.to_string();
    let outbound = match decision {
        IntentDecisionDto::Accept { device_id } => {
            orchestrate::decide(
                plugins,
                &state.board,
                local,
                &id,
                None,
                Some(&device_id.to_string()),
                synra_store::sqlite::now_ms(),
            )
            .map_err(|error| as_ipc(&error))?
            .outbound
        }
        // The device the kernel settled on, which is this one: the shell has no peers it can dispatch to.
        IntentDecisionDto::AcceptDefault => {
            orchestrate::decide(
                plugins,
                &state.board,
                local,
                &id,
                None,
                None,
                synra_store::sqlite::now_ms(),
            )
            .map_err(|error| as_ipc(&error))?
            .outbound
        }
        // A refusal is the same move as a cancellation and is recorded the same way, with the user's own reason when
        // they gave one. A remote intent produces a withdrawal envelope, which `deliver_decided` sends without waiting.
        IntentDecisionDto::Decline { reason } => {
            let why = reason.as_deref().unwrap_or("the user declined");
            // The local device is named because a cancellation of a **dispatched** intent has to say who it is from, and
            // an intent raised here records no origin.
            orchestrate::cancel(&state.board, local, &id, why)
                .map_err(|error| as_ipc(&error))?
                .outbound
        }
        // ★ `IntentDecisionDto` is `#[non_exhaustive]`, so a decision added later lands here rather than failing to
        // compile. It is refused rather than treated as a decline: an interface newer than this build is asking for
        // something this build cannot do, and recording that as "the user said no" would put a decision in the audit
        // trail that nobody made.
        _ => {
            return Err(
                IpcError::unknown(synra_protocol::ErrorCode::Unsupported).with_params(
                    serde_json::json!({ "reason": "this build does not know that decision" }),
                ),
            );
        }
    };

    // ★ Announced before the envelope leaves: a remote invocation may spend the whole wait on the wire, and the card has
    // to say "dispatching" while that happens. A refused delivery still announces, because the decision itself landed.
    announce(state, intent_id);
    // ★ Written down before the envelope leaves as well: a remote dispatch is exactly the intent a restart would otherwise
    // lose, and the peer is still waiting on it. The write happens after the decision, so it records `dispatching` rather
    // than the state before it.
    checkpoint(state, intent_id);
    if let Some(outbound) = &outbound {
        match crate::commands::intent_remote::deliver_decided(state, outbound) {
            Ok(Some(reply)) => {
                // The far side's outcome is the intent's outcome. Applying it is what turns `dispatching` into
                // `succeeded`/`failed`; a reply this device is not waiting on is ignored by `apply_result` itself.
                orchestrate::apply_result(&state.board, &reply).map_err(|error| as_ipc(&error))?;
                announce(state, intent_id);
                checkpoint(state, intent_id);
            }
            // No answer inside the wait. The action may still be running over there, so nothing is invented: the intent
            // stays dispatching and the listener applies a late result.
            Ok(None) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Abandons an intent.
///
/// # What a remote intent gets
///
/// A withdrawal envelope, sent by [`crate::commands::intent_remote::deliver_decided`]. It is not answered — the protocol
/// has no "I stopped" reply — so this does not wait. The local cancellation has already happened when the send is
/// attempted: a failed delivery does not bring the intent back, and the error says the other device was not told rather
/// than implying nothing happened.
///
/// # Errors
///
/// [`IpcError`] when the identifier is not one or names an intent this shell never raised, and when the withdrawal could
/// not be delivered. The intent is cancelled either way.
pub fn intent_cancel(state: &AppState, intent_id: IntentId) -> Result<(), IpcError> {
    let local = state.identity.lock().device_id;
    let change = orchestrate::cancel(
        &state.board,
        local,
        &intent_id.to_string(),
        "the user cancelled it",
    )
    .map_err(|error| as_ipc(&error))?;
    // The panel shows a cancelled intent as cancelled rather than making it vanish: the board keeps it (`intent_list` is
    // `all`, not the live ones), so the announcement is the same announcement a decision makes.
    announce(state, intent_id);
    checkpoint(state, intent_id);
    if let Some(outbound) = &change.outbound {
        crate::commands::intent_remote::deliver_decided(state, outbound)?;
    }
    Ok(())
}
