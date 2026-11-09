//! The plugin page's two calls: "I have drawn" and "do this for me".
//!
//! # Why one takes a label and the other a token
//!
//! `plugin_ui_ready` is the page's first word, and it presents the label the host gave its window: `validate` needs the
//! nonce, the claiming plugin and the version, and only the host's own table can supply the last two. The token is still
//! checked — it is the entry's own nonce.
//!
//! `plugin_ui_rpc` presents the private token. Public window labels use an independent document id and never contain
//! that token. Each request rechecks the committed authority revision as well as the nonce; neither a stale document
//! nor a disabled plugin regains access by enabling or reinstalling the same version.
//!
//! # What it refuses, and what it deliberately does not say
//!
//! A token nobody holds, and a token the registry rejects, are both refusals with the same code and the same parameters.
//! Saying which of the two applies would let a page probe for values that exist — the enumeration a token is supposed to
//! prevent.

use crate::state::AppState;
use synra_capability::Capability;
use synra_ipc::{IpcError, PluginUiOp};
use synra_platform::Platform as _;
use synra_plugin_ui::ValidatedSession;
use synra_protocol::ErrorCode;
use synra_protocol::ids::PluginId;

mod authority;
mod origin;

const MAX_DEVICE_TARGETS: usize = 256;

mod platform;
mod rpc;
mod storage;
mod transfer;
mod window;

#[cfg(test)]
pub(crate) use platform::call_error;
pub(crate) use rpc::*;
pub(crate) use storage::*;
pub(crate) use transfer::*;
#[cfg(feature = "tauri-shell")]
pub(crate) use window::stays_on_page;
#[cfg(test)]
pub(crate) use window::*;

pub use rpc::{plugin_ui_ready, plugin_ui_rpc, plugin_ui_rpc_arc};
pub use transfer::Uploads;
#[cfg(feature = "tauri-shell")]
pub use window::serve;
pub use window::{
    OpenDecision, OpenRequest, forget_window, open_request, page_resource, split_ui_url,
    stale_windows, ui_file, window_label_prefix,
};

#[cfg(test)]
mod tests;
