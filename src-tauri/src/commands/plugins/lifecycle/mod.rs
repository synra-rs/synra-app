use super::*;

/// Runtime lifecycle transitions.
mod apply;
/// Consent recording for installed plugins.
mod consent;
/// Discovery, startup loading and revocations.
mod discovery;
/// Peer package reconciliation.
mod install;
/// Publisher trust decisions.
mod publishers;
/// Reloading an already installed plugin.
mod reload;

pub(crate) use apply::*;
pub use consent::*;
pub(crate) use discovery::*;
#[cfg(feature = "tauri-shell")]
pub(crate) use install::{install_package, reconcile_after_install};
pub use publishers::*;
pub use reload::*;
