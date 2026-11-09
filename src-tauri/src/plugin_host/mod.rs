//! What a plugin's host calls reach, in the desktop shell.
//!
//! # Why this module had to exist
//!
//! Because the shell's plugin set was built with **no host at all**. `PluginSet::new` took one argument when this file
//! was written and two when the capability gate was finally connected to anything — so `main.rs` stopped compiling and
//! nothing said so, because the check being run was `cargo test --lib` and this code lives in the binary.
//!
//! The signature was not an accident: an instance with no session refuses every host call, which is what made the gate
//! inert for months. Requiring the host made that state unrepresentable, and the cost was that a shell which had never
//! supplied one had to say what its host was.
//!
//! # What it serves, and what it refuses
//!
//! The same rule as the daemon's: answer where this build genuinely knows, refuse with a code where it does not. The
//! difference is the point of having two — a shell has a desktop, so `platform:*` reaches
//! [`CommandPlatform`](synra_platform::CommandPlatform), which opens a browser, reads the clipboard and shows a
//! notification for real. The daemon is headless and refuses the same calls; `synra-platform` had the desktop
//! implementation, tested, and **no caller** until this file.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};

use parking_lot::Mutex;

use synra_capability::CapabilitySet;
use synra_platform::Platform as _;
use synra_plugin_host::host_calls::{CallError, Host, PlatformAction};
use synra_protocol::{
    DeviceId, PluginId,
    envelope::{Envelope, OutboundSpec},
    event::EventName,
};
use synra_store::SynraDb;

/// Registered desktop applications.
mod apps;
/// Desktop host state and grant-aware helpers.
mod desktop;
/// Host-call implementation.
mod host;

use apps::{app_capability, open_registered_app, registered_apps};
pub use desktop::DesktopHost;
#[cfg(test)]
use desktop::host_of;
use desktop::{failed, fetcher_for, not_implemented, platform_refusal};
pub use host::audit_sink;

#[cfg(test)]
mod clipboard_tests;
#[cfg(test)]
mod event_tests;
#[cfg(test)]
mod scoped_fetch_tests;
#[cfg(test)]
mod tests;
