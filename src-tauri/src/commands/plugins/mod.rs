//! The plugins this device has.
//!
//! # Installed, enabled, trusted and loaded are four different questions
//!
//! The list the interface shows is of *installed* plugins — what is on disk. Whether it is **enabled** is a decision the
//! user made. Whether it is **trusted** is a decision the user made about what it may do, and it is stored separately on
//! purpose: re-consenting after an upgrade is not the same act as switching a plugin on, and a plugin that silently
//! starts running is exactly what consent exists to prevent. Whether it is **loaded** is what this process managed to
//! do with its engine.
//!
//! `PluginSnapshot::state` carries the kernel's own answer to "where is it in its life", and the shell does not derive a
//! second state from the other three flags — the design document (`08` §2.2) already exists so that this rule has one
//! implementation, and a second one in the shell is where the two start disagreeing.
//!
//! # What `plugins_list` never does
//!
//! It never filters out a plugin because it failed to load. A plugin that is installed and broken is the one a user most
//! needs to see: they will go looking for the setting to remove it, and if it is not on the screen they will file a bug
//! about it having vanished.

use synra_ipc::{
    AuditEntryDto, CatalogItem, CatalogQuery, InstallPreviewDto, InstallReport, InstallSourceDto,
    IpcError, PluginHealthDto, PluginSnapshot, RemoveReport, UpdateInfo,
};
use synra_plugin_api::Manifest;
use synra_protocol::PluginId;
use synra_store::PluginUpdateCheck;

use crate::state::{AppState, internal};

mod audit;
mod catalog;
mod install;
mod lifecycle;
mod list;

#[cfg(test)]
pub(crate) use audit::{AUDIT_RETENTION_KEY, record_card};
pub(crate) use audit::{directory_size, prune_audit, record_install};
pub use audit::{plugins_audit, plugins_audit_clear};
pub(crate) use catalog::announce_updated;
pub use catalog::{
    plugins_catalog_search, plugins_check_update, plugins_preview, plugins_update, plugins_updates,
};
pub(crate) use install::{check_local_update, local_directory_source};
pub(crate) use install::{
    complete_update, install_directory, migrate_and_unload, reload_directory, update_directory,
};
pub(crate) use lifecycle::{apply_lifecycle, apply_unload};
#[cfg(test)]
pub(crate) use list::life_state;
pub(crate) use list::{announce, parse, read_manifest, read_manifest_text, set_enabled, snapshot};

pub use install::{plugins_install, plugins_uninstall};
#[cfg(feature = "tauri-shell")]
pub(crate) use lifecycle::install_package;
#[cfg(feature = "tauri-shell")]
pub(crate) use lifecycle::reconcile_after_install;
pub(crate) use lifecycle::revocations;
#[cfg(any(feature = "tauri-shell", test))]
pub(crate) use lifecycle::{discover_plugins, load_installed_plugins};
pub use lifecycle::{
    plugins_consent_respond, plugins_publisher_decide, plugins_publishers, plugins_reload,
};
pub use list::{plugins_disable, plugins_enable, plugins_list};

#[cfg(test)]
mod tests;
