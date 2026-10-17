//! Routing a share, answering the prompt, and cancelling.
//!
//! # ★ Why these four were missing, and what was actually wrong
//!
//! The registration list carried them with the reason *"needs the intent board behind `AppState`"*, and that was false:
//! `AppState::board` has been there since the state was written. What was missing was the **orchestration** — the code
//! that raises an intent, asks every plugin, settles the window, applies the answer and runs what was chosen lived in
//! `synra-daemon/src/runtime/intent.rs`, **inside a binary**, where a second host could not reach it.
//!
//! It lives in `synra_core::dispatch::orchestrate` now, and these commands are the thin layer that turns interface DTOs
//! into calls and the answers back into DTOs. That is the whole of what a host should have to write.
//!
//! # Where the interface's vocabulary stops matching the kernel's
//!
//! Two places, both of them the interface wanting something the kernel does not have:
//!
//! * **A candidate always names a device.** [`IntentCandidate`] requires one; the kernel's candidate may have *no*
//!   opinion about where to run, which is the state that produces the "where?" prompt. See [`candidate_card`].
//! * **A share carries no MIME type.** The interface's `Share` has text and an optional title; the kernel's `ShareItem`
//!   has a MIME, and a plugin deciding "can I handle this?" is entitled to know whether it is looking at a URI list or a
//!   sentence. The shell sends [`TEXT_MIME`] and the mismatch is recorded rather than papered over — see
//!   [`item_for`].
//!
//! # ★ The fifth command, which the interface asked for before the contract had it
//!
//! [`intent_list`] is here because `HomePage.vue`'s "received" panel wanted the intents in flight and **no command
//! answered them**: the page called `intent_snapshot` with no identifier — which the bridge refuses, because the wrapper
//! requires one, so the user read "The interface could not reach the kernel" for a command the kernel never ran — and it
//! read `answer.value.intents`, a field no DTO has.
//!
//! The capability was never missing. `Board::all()` has been there since the board was written, and its own documentation
//! says what it is for: persisting has to save what *changed*, which includes an intent that has just ended. What was
//! missing was a **name** for it. The panel's shape is a bare array (`Many`), like `plugins_list` and `transfer_list` —
//! not an envelope, which is the drift `src/ipc/commands.test.ts` exists to catch.

use synra_core::dispatch::orchestrate::{self, RouteError};
use synra_core::messaging::TEXT_MIME;
use synra_ipc::{IntentCandidate as CandidateDto, IntentDecisionDto, IntentSnapshot, IntentSourceDto, IpcError};
use synra_protocol::ids::IntentId;
use synra_protocol::inbound::ShareItem;

use crate::state::AppState;

/// A library failure as the interface reports it.
///
/// One line, and that is why the library carries the contract's own codes: a host that had to map a bespoke enum would
/// be inventing the vocabulary back.
fn as_ipc(error: &RouteError) -> IpcError {
    IpcError::unknown(error.code).with_params(serde_json::json!({ "reason": error.message }))
}

/// Reads a plugin set, or refuses the way every plugin command does.
fn plugins(state: &AppState) -> Result<&std::sync::Arc<parking_lot::Mutex<synra_core::dispatch::PluginSet>>, IpcError> {
    state.plugin_set()
}

/// The share item a source describes.
///
/// # Why two of the four sources are refused
///
/// Because the kernel has nothing to route them to, and saying so is better than routing something else:
///
/// * `ShareFiles` needs transfers, and the interface has no path that uploads a file first. A `ShareItem` with an empty
///   transfer list would tell every plugin "the user shared nothing", which is a lie they would act on.
/// * `PluginAction` names a plugin and an action directly, so there is nothing to match — the board's own `raise` skips
///   matching for a `Direct` source. That path exists in the kernel and this command does not go through it, because
///   `orchestrate::dispatch` is the *share* entry point; invoking a named action is a different call than routing one.
fn item_for(state: &AppState, source: &IntentSourceDto) -> Result<ShareItem, IpcError> {
    let from = state.identity.lock().device_id;
    let refused = |what: &str| {
        IpcError::unknown(synra_protocol::ErrorCode::Unsupported)
            .with_params(serde_json::json!({ "reason": what }))
    };
    match source {
        IntentSourceDto::Share { text, title: _ } => Ok(ShareItem {
            from,
            // ★ See the module docs: the interface's `Share` has no MIME, so this is what the shell sends. A launcher
            // plugin that cares whether its input is a link should say so by its own declaration rather than by the
            // shell guessing from the text, which is how a string that merely looks like a URL gets opened.
            mime: TEXT_MIME.to_owned(),
            text: Some(text.clone()),
            transfers: Vec::new(),
            context_id: None,
        }),
        IntentSourceDto::Clipboard => {
            // The clipboard's content is read here rather than by a plugin, because a plugin that could read the
            // clipboard on its own would be able to read it whenever it liked. The user asked for this share, so the
            // shell reads it once, for this item.
            use synra_platform::Platform as _;
            // Made here rather than held on `AppState`: `CommandPlatform` is a desktop enum and a runner with no state,
            // and the only thing on it that accumulates is the notification rate, which reading the clipboard does not
            // touch. A field on the state would be one more thing for every command to thread.
            let text = synra_platform::CommandPlatform::default()
                .read_clipboard()
                .map_err(|error| {
                    IpcError::unknown(synra_protocol::ErrorCode::Unsupported)
                        .with_params(serde_json::json!({ "reason": error.to_string() }))
                })?;
            // ★ `None` is an empty clipboard, which is not a share with no content.
            //
            // A `ShareItem` whose text is `None` tells every plugin the clipboard held nothing *of a kind it can use*,
            // which is a different fact from "the clipboard is empty" and one a plugin would act on differently. The
            // user asked to share what they had copied; there was nothing, and that is what they are told.
            let Some(text) = text else {
                return Err(refused("the clipboard is empty"));
            };
            Ok(ShareItem {
                from,
                mime: TEXT_MIME.to_owned(),
                text: Some(text),
                transfers: Vec::new(),
                context_id: None,
            })
        }
        IntentSourceDto::ShareFiles { .. } => Err(refused(
            "sharing files needs a transfer first, and no interface starts one yet",
        )),
        IntentSourceDto::PluginAction { .. } => Err(refused(
            "invoking a named plugin action is not the share entry point; nothing routes it yet",
        )),
        // ★ `IntentSourceDto` is `#[non_exhaustive]`. A source this build does not know is refused rather than
        // routed as a share: guessing what the user meant is how the wrong thing gets opened.
        _ => Err(refused("this build does not know that kind of share")),
    }
}

/// One candidate, as the interface shows it.
///
/// ★ **A candidate that named no device is reported as running here**, which is the kernel's own meaning: with no
/// suggestion the default is this machine, and the "where?" question exists precisely because that default may not be
/// what the user wants. The interface's `IntentCandidate` requires a device, so there is no way to say "no opinion" here
/// — and reporting a device the plugin did not name is a smaller distortion than reporting none at all, which the shape
/// does not permit.
fn candidate_card(state: &AppState, candidate: &orchestrate::CandidateView) -> CandidateDto {
    let local = state.identity.lock().device_id;
    let device_id = candidate.suggested_target.unwrap_or(local);
    CandidateDto {
        plugin_id: candidate.plugin_id.clone(),
        title: candidate.title.clone(),
        device_id,
        // This device is reachable by definition; a peer is looked up. Answered rather than assumed, because a chooser
        // that offers an offline device is a chooser whose first choice fails.
        online: device_id == local
            || state
                .db()
                .paired_devices()
                .unwrap_or_default()
                .iter()
                .any(|row| row.device_id == device_id.to_string() && row.is_usable()),
    }
}

/// Hands the kernel something to route.
///
/// # Errors
///
/// [`IpcError`] when the source is one this build cannot route, or when there is no plugin engine.
pub fn intent_dispatch(state: &AppState, source: &IntentSourceDto) -> Result<IntentId, IpcError> {
    let plugins = plugins(state)?;
    let item = item_for(state, source)?;
    let routed = orchestrate::dispatch(plugins, &state.board, &item, synra_store::sqlite::now_ms())
        .map_err(|error| as_ipc(&error))?;
    // ★ The window learns an intent exists without asking. The snapshot is read back from the board rather than built
    // from `routed`: the board is what `intent_list` will answer with, and a card assembled here would be a second
    // description of the same intent — the one that disagrees the first time the two are computed differently.
    announce(state, routed.intent_id);
    Ok(routed.intent_id)
}

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

/// One intent, as the interface is shown it.
///
/// ★ Both reads go through here, and that is the point of it being a function: `intent_list` and `intent_snapshot` answer
/// about the same thing, so a field added to the DTO cannot appear in one answer and be missing from the other.
///
/// # Errors
///
/// [`IpcError`] when the identifier names an intent this shell never raised.
#[allow(clippy::result_large_err)]
/// Announces an intent's current state, if the board can still answer for it.
///
/// Silent when it cannot: the board losing an intent between the change and the announcement is a race the caller cannot
/// repair, and publishing an error is not what an event is for. The interface's next `intent_list` is the truth either
/// way, so a missed announcement costs one stale list rather than a wrong one.
fn announce(state: &AppState, intent_id: IntentId) {
    if let Ok(snapshot) = snapshot_of(state, intent_id) {
        state.publish(synra_ipc::event::KernelEvent::IntentUpdated(Box::new(snapshot)));
    }
}

fn snapshot_of(state: &AppState, intent_id: IntentId) -> Result<IntentSnapshot, IpcError> {
    let view = orchestrate::snapshot(&state.board, &intent_id.to_string()).map_err(|error| as_ipc(&error))?;
    Ok(IntentSnapshot {
        intent_id: view.intent_id,
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
        error: None,
        // The board records when an intent was created; the view does not carry it, so this is the moment it was read
        // rather than the moment it began — see the note in doc 08's checklist.
        started_at_ms: synra_store::sqlite::now_ms(),
    })
}

/// Applies the user's answer, and runs what they chose.
///
/// # Errors
///
/// [`IpcError`] when the intent is unknown or is not in a state that can be decided, or when the chosen device is not
/// this one — running an action on another device is a message this build does not send.
pub fn intent_decide(
    state: &AppState,
    intent_id: IntentId,
    decision: &IntentDecisionDto,
) -> Result<(), IpcError> {
    let plugins = plugins(state)?;
    let local = state.identity.lock().device_id;
    let id = intent_id.to_string();
    let outcome = match decision {
        IntentDecisionDto::Accept { device_id } => orchestrate::decide(
            plugins,
            &state.board,
            local,
            &id,
            None,
            Some(&device_id.to_string()),
            synra_store::sqlite::now_ms(),
        )
        .map(|_| ())
        .map_err(|error| as_ipc(&error)),
        // The device the kernel settled on, which is this one: the shell has no peers it can dispatch to.
        IntentDecisionDto::AcceptDefault => orchestrate::decide(
            plugins,
            &state.board,
            local,
            &id,
            None,
            None,
            synra_store::sqlite::now_ms(),
        )
        .map(|_| ())
        .map_err(|error| as_ipc(&error)),
        // A refusal is the same move as a cancellation and is recorded the same way, with the user's own reason when
        // they gave one.
        IntentDecisionDto::Decline { reason } => {
            let why = reason.as_deref().unwrap_or("the user declined");
            // The local device is named because a cancellation of a **dispatched** intent has to say who it is from, and
            // an intent raised here records no origin. See `cannot_send`.
            refuse_if_remote(&orchestrate::cancel(&state.board, local, &id, why).map_err(|error| as_ipc(&error))?)
        }
        // ★ `IntentDecisionDto` is `#[non_exhaustive]`, so a decision added later lands here rather than failing to
        // compile. It is refused rather than treated as a decline: an interface newer than this build is asking for
        // something this build cannot do, and recording that as "the user said no" would put a decision in the audit
        // trail that nobody made.
        _ => Err(IpcError::unknown(synra_protocol::ErrorCode::Unsupported)
            .with_params(serde_json::json!({ "reason": "this build does not know that decision" }))),
    };

    // ★ Announced only when the decision landed: a refused decision changed nothing, and publishing the intent's
    // unchanged state would tell the panel something happened.
    outcome?;
    announce(state, intent_id);
    Ok(())
}

/// Refuses when a move produced a message this shell cannot send.
///
/// # ★ Why the shell refuses rather than dropping the envelope
///
/// The orchestration answers a remote target with an `Outbound` — the envelope a host must put on a link — and this shell
/// has **no transport at all**: no listener, no dial, no `synra-transfer`. Dropping the envelope would leave the far side
/// running work the user withdrew, or waiting for an action that was never sent, while this side reported success.
///
/// So it is refused, and the refusal names the missing half. What has already happened locally is not undone — the intent
/// *is* cancelled here — and the sentence says so, because a refusal that reads as "nothing happened" would send the user
/// looking for an intent that is already gone.
fn refuse_if_remote(change: &orchestrate::StateChange) -> Result<(), IpcError> {
    match &change.outbound {
        None => Ok(()),
        Some(outbound) => Err(IpcError::unknown(synra_protocol::ErrorCode::Unsupported).with_params(
            serde_json::json!({
                "reason": format!(
                    "{} was not told: this shell has no transport, so it cannot send the message that would stop the \
                     work it is running. The intent is cancelled here.",
                    outbound.to
                ),
            }),
        )),
    }
}

/// Abandons an intent.
///
/// # Errors
///
/// [`IpcError`] when the identifier is not one or names an intent this shell never raised.
pub fn intent_cancel(state: &AppState, intent_id: IntentId) -> Result<(), IpcError> {
    let local = state.identity.lock().device_id;
    refuse_if_remote(
        &orchestrate::cancel(&state.board, local, &intent_id.to_string(), "the user cancelled it")
            .map_err(|error| as_ipc(&error))?,
    )?;
    // The panel shows a cancelled intent as cancelled rather than making it vanish: the board keeps it (`intent_list` is
    // `all`, not the live ones), so the announcement is the same announcement a decision makes.
    announce(state, intent_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_with_no_mime_is_sent_as_text() {
        // ★ The mismatch the module docs record. The interface's `Share` has no MIME and the kernel's item must, so the
        // shell picks one — and `text/plain` is the honest choice for "the user shared text". Sending a URI list because
        // the text happened to look like a URL is the guess this test exists to prevent.
        let state = crate::testing::state();
        let item = item_for(
            &state,
            &IntentSourceDto::Share { text: "https://example.com".to_owned(), title: None },
        )
        .expect("text is routable");

        assert_eq!(item.mime, "text/plain");
        assert_eq!(item.text.as_deref(), Some("https://example.com"));
        assert!(item.transfers.is_empty());
    }

    #[test]
    fn the_two_sources_this_build_cannot_route_are_refused() {
        // A file share needs a transfer the interface never starts, and a named plugin action is not the share entry
        // point. Both are refused rather than routed as something else: a `ShareItem` with no transfers would tell every
        // plugin the user shared nothing, which they would act on.
        let state = crate::testing::state();

        assert!(item_for(&state, &IntentSourceDto::ShareFiles { paths: vec!["/a".to_owned()] }).is_err());
        assert!(
            item_for(
                &state,
                &IntentSourceDto::PluginAction {
                    plugin_id: synra_protocol::PluginId::parse("@acme/notes").expect("a plugin id"),
                    action: "open".to_owned(),
                    args: serde_json::json!({}),
                },
            )
            .is_err()
        );
    }

    #[test]
    fn announcing_an_intent_the_board_cannot_answer_for_publishes_nothing() {
        // ★ The decision the helper documents: a board that lost the intent between the change and the announcement is a
        // race no caller can repair, and publishing an **error** is not what an event is for. A missed announcement costs
        // one stale list; a made-up one costs a wrong one.
        let state = crate::testing::state();
        let subscriber = state.events.subscribe();

        announce(&state, synra_protocol::ids::IntentId::new());

        assert!(state.events.take(subscriber).is_none(), "nothing was announced, because nothing was known");
    }

    #[test]
    fn a_command_without_an_engine_says_which_thing_is_missing() {
        // The shell is constructible without a plugin engine, and every intent command then refuses with the reason
        // rather than an empty answer. "No plugins are installed" and "this build cannot run them" are different facts
        // and a user acts on them differently.
        let state = crate::testing::state();
        let refused = intent_dispatch(
            &state,
            &IntentSourceDto::Share { text: "hi".to_owned(), title: None },
        )
        .expect_err("this fixture has no engine");

        assert_eq!(refused.code, synra_protocol::ErrorCode::Unsupported);
    }

    #[test]
    fn an_unknown_intent_is_refused_rather_than_answered_with_an_empty_one() {
        let state = crate::testing::state();
        let unknown = IntentId::new();

        assert!(intent_snapshot(&state, unknown).is_err(), "no such intent is not an empty snapshot");
        assert!(intent_cancel(&state, unknown).is_err());
    }

    #[test]
    fn an_empty_board_is_an_empty_list_rather_than_a_refusal() {
        // The panel this command exists for has nothing to show on a device that has never received anything, and
        // "nothing" is an answer. A refusal here would put an error on the home page of every fresh install.
        let state = crate::testing::state();

        assert!(intent_list(&state).expect("an empty board is an empty list").is_empty());
    }

    #[test]
    fn the_list_and_the_single_read_describe_an_intent_the_same_way() {
        // ★ Both commands build their answer through `snapshot_of`, and this is what says so: one mapping means a field
        // added to the DTO cannot appear in one answer and be missing from the other.
        let state = crate::testing::state();
        let id = raise_an_intent(&state);

        let listed = intent_list(&state).expect("the board can be read");
        assert_eq!(listed.len(), 1, "the one intent that was raised is the one that is listed");
        assert_eq!(listed[0], intent_snapshot(&state, id).expect("the same intent"));
    }

    /// Puts one intent on the board directly.
    ///
    /// `orchestrate::dispatch` is the share entry point and needs a plugin engine, which this fixture deliberately has none
    /// of. `intent_list` reads the board, so the board is what the test writes — and `raise` is the kernel's own way to
    /// write one, so what is listed is a real intent rather than a row shaped for the test.
    fn raise_an_intent(state: &AppState) -> IntentId {
        use synra_core::intent::{Choice, IntentSource};

        let item = ShareItem {
            from: state.identity.lock().device_id,
            mime: TEXT_MIME.to_owned(),
            text: Some("https://example.com".to_owned()),
            transfers: Vec::new(),
            context_id: None,
        };
        let (id, _trace, created) = state.board.lock().raise(
            IntentSource::Share { item, source_app: None },
            Choice::Automatic,
            None,
            None,
            synra_store::sqlite::now_ms(),
        );

        assert!(created, "a request that was never raised before raises a new intent");
        id
    }
}
