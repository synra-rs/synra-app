//! The commands the interface calls.
//!
//! # What every command in here has in common
//!
//! It takes `&AppState` and returns `Result<T, IpcError>`. It does not take a `State<'_, _>`, does not import Tauri,
//! and does not touch a window. That is what makes every command in this module unit-testable, and it is why `main.rs`
//! exists as a separate file rather than as a function at the bottom of this one.
//!
//! # The one rule the commands share
//!
//! **A command either answers or says why it cannot, and never invents an answer.** An empty list of devices means no
//! devices; a refusal means this build cannot tell. The interface renders the two differently and the difference is the
//! difference between a user looking for a pairing problem and a user filing a bug, so no command is allowed to answer
//! with the wrong one to be convenient.

pub mod config;
pub mod devices;
pub mod diag;
pub mod identity;
pub mod intent;
pub mod plugins;

// ★ No flat re-export list, and its absence is deliberate.
//
// There was one — `pub use plugins::{plugins_disable, plugins_enable, plugins_list}` and three more lines like it — and
// it was **both unused and incomplete**: nothing outside this module ever wrote `commands::plugins_list`, because
// `main.rs`'s wrappers name the full path, and the list had quietly fallen behind by seven commands
// (`diag_logs_tail`, `plugins_consent_respond`, `plugins_audit` and the four intent commands).
//
// A list that nobody reads and that is missing a third of its entries is worse than no list: the next person to add a
// command sees it, assumes it is the inventory, and adds to a claim that is already wrong.
