//! Installing a plugin from a registry, the smallest honest path.
//!
//! # What is configured
//!
//! One key, `plugins.registry`, holding the base URL of an npm-compatible registry. Absent means this device has not
//! chosen one, and the answer is `Unsupported` rather than a network attempt at a guess — a shell that dialled a default
//! somebody else chose would be a shell that phones home.
//!
//! # What is fetched
//!
//! The package's **packument**, then the tarball its chosen version names. Both go through [`synra_fetch`]'s guarded
//! client, so the host, the scheme, every redirect and the response ceiling are the same rules a plugin's network
//! capability obeys. The tarball must be on the configured origin: a registry that points a package at another host is
//! asking this device to fetch somewhere nobody configured, which is refused rather than followed.
//!
//! # What is heard
//!
//! The tarball is verified and opened by [`synra_core::install::unpack_published_tarball`] into a directory under the
//! cache — the same function the daemon's registry install calls — and then the **existing** install pipeline runs over
//! it. Signature checks, archive limits, the manifest and the store all stay in one place; this module is transport.
//!
//! # What the preview is
//!
//! `plugins_preview` reads the packument's inline manifest and shows its permissions **before** the tarball is fetched.
//! It is a question, not an approval: the signature is checked on the bytes, and the plugin still lands switched off.
//!
//! # What this is not
//!
//! A catalog. `plugins_catalog_search` still answers from what is installed, because a search endpoint is a different
//! protocol shape (16 §4.3) and pretending one request is a search would be a worse answer than the filter.

use std::collections::BTreeMap;

use serde::Deserialize;
use synra_core::network::{IpPolicy, NetworkGuard};
use synra_fetch::{Client, Limits as FetchLimits, Request};
use synra_ipc::dto::{
    CatalogItem, CatalogQuery, CatalogSort, InstallPreviewDto, InstallReport, InstallSourceDto,
    UpdateInfo,
};
use synra_protocol::ErrorCode;
use synra_protocol::PluginId;
use synra_registry::{Packument, Version};

use crate::state::AppState;

/// The configuration key holding the registry's base URL.
pub use synra_core::plugin_updates::REGISTRY_KEY;

/// How large a response may be when a package is being downloaded.
///
/// The design's package ceiling is 64 MiB (16 §2.1); the client's ceiling is one byte more so an artefact that is exactly
/// the limit is accepted, and the archive's own limits do the rest once it is in memory.
const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024 + 1;

/// A static index this process has already parsed, and the validators its host sent.
#[derive(Debug, Clone)]
pub struct CachedDocument {
    /// The `ETag` the host answered with, for `If-None-Match`.
    pub etag: Option<String>,
    /// The `Last-Modified` the host answered with, for `If-Modified-Since`.
    pub last_modified: Option<String>,
    /// The parsed document, which a 304 says is still current.
    pub document: Packument,
    /// The publisher fingerprints the directory listed, carried with the document so a 304 keeps them.
    pub publishers: std::collections::BTreeMap<String, String>,
    /// When `Cache-Control: max-age` says the document should be re-read; `None` means the host gave no lifetime.
    ///
    /// ★ `None` is not "expired": a host that sent only a validator is revalidated on every use, which is what an ETag is
    /// for. The lifetime matters for a host that sent no validator at all — without it the entry would never be refreshed.
    pub expires_at_ms: Option<i64>,
}

/// How a registry answers "what versions exist?".
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Kind {
    /// An npm-compatible registry: a packument at `{origin}/{id}`.
    Npm,
    /// A static host: one `index.json` per package at `{origin}/{id}.json` (16 §4.4).
    Static,
}

/// A registry address, split into what the guard and the URLs need.
#[derive(Debug, Clone)]
pub(crate) struct Base {
    /// `scheme://authority`, plus the path prefix for a static host, without a trailing slash.
    origin: String,
    /// The host alone, as a capability grants it.
    host: String,
    /// Whether the scheme is `https`.
    secure: bool,
    /// Which document the address serves.
    kind: Kind,
}

mod cache;
mod catalog;
mod install;
mod transport;

pub(crate) use cache::*;
pub(crate) use catalog::*;
pub(crate) use install::*;
pub(crate) use transport::*;

pub use cache::prune_documents;
pub use catalog::{check_update, packument, search};
pub use install::{install, install_from_source, install_source, preview, update};

#[cfg(test)]
mod tests;
