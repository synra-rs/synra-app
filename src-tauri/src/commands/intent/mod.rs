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
use synra_ipc::{
    IntentCandidate as CandidateDto, IntentDecisionDto, IntentSnapshot, IntentSourceDto, IpcError,
};
use synra_protocol::ids::IntentId;
use synra_protocol::inbound::ShareItem;

use crate::state::AppState;

mod decision;
mod route;
mod snapshot;

#[cfg(test)]
mod tests;

pub use decision::{intent_cancel, intent_decide};
pub use route::intent_dispatch;
pub use snapshot::{intent_list, intent_snapshot};

#[cfg(test)]
use route::item_for;
use route::{as_ipc, candidate_card, plugins};
pub(crate) use snapshot::{announce, checkpoint};
