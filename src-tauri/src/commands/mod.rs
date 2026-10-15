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

pub mod devices;
pub mod identity;
pub mod plugins;

pub use devices::{devices_forget, devices_scan_cancel, devices_scan_start, devices_snapshot};
pub use identity::{identity_get, identity_set_display_name};
pub use plugins::{plugins_disable, plugins_enable, plugins_list};
