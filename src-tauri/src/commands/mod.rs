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
pub mod plugin_ui;
pub mod plugins;
/// The transfers in flight: the registry the worker writes and  ransfer_list reads.
pub mod transfers;

// ★ These are re-exports, not an inventory.
//
// They were removed once on the grounds that nothing used them, and that was wrong: `diag/tests.rs` calls
// `crate::commands::config_set`. What *was* true is that the list had fallen seven commands behind — `diag_logs_tail`,
// `plugins_consent_respond`, `plugins_audit` and the four intent commands — so a reader could take it for the full set
// and be wrong.
//
// **The inventory is not here.** It is `main.rs`'s `generate_handler!`, and `src/ipc/registration.test.ts` is what checks
// that list against the 40 commands the kernel declares. This is a convenience re-export kept complete so it cannot
// mislead, and the place to look for "what does the shell implement" is that test.
pub use config::{config_get, config_set};
pub use devices::{devices_forget, devices_scan_cancel, devices_scan_start, devices_snapshot};
pub use diag::{diag_export, diag_logs_tail, diag_metrics};
pub use identity::{identity_get, identity_set_display_name};
pub use intent::{intent_cancel, intent_decide, intent_dispatch, intent_list, intent_snapshot};
pub use plugins::{plugins_audit, plugins_consent_respond, plugins_disable, plugins_enable, plugins_list};
