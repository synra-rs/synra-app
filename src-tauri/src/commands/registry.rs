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
//! The tarball is opened by [`synra_registry::extract_tgz`] into a directory under the cache, and then the **existing**
//! install pipeline runs over it. Signature checks, archive limits, the manifest and the store all stay in one place; this
//! module is transport.
//!
//! # What this is not
//!
//! A catalog. `plugins_catalog_search` still answers from what is installed, because a search endpoint is a different
//! protocol shape (16 §4.3) and pretending one request is a search would be a worse answer than the filter.

use std::collections::BTreeMap;

use synra_core::network::{IpPolicy, NetworkGuard};
use synra_fetch::{Client, Limits as FetchLimits, Request};
use synra_ipc::dto::{InstallReport, InstallSourceDto};
use synra_protocol::ErrorCode;
use synra_registry::{Limits as ArchiveLimits, Packument, extract_tgz};

use crate::state::AppState;

/// The configuration key holding the registry's base URL.
pub const REGISTRY_KEY: &str = "plugins.registry";

/// How large a response may be when a package is being downloaded.
///
/// The design's package ceiling is 64 MiB (16 §2.1); the client's ceiling is one byte more so an artefact that is exactly
/// the limit is accepted, and the archive's own limits do the rest once it is in memory.
const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024 + 1;

/// A registry address, split into what the guard and the URLs need.
struct Base {
    /// `scheme://authority`, without a trailing slash.
    origin: String,
    /// The host alone, as a capability grants it.
    host: String,
    /// Whether the scheme is `https`.
    secure: bool,
}

/// The configured registry, or a refusal naming the key.
fn base(state: &AppState) -> Result<Base, synra_ipc::IpcError> {
    let rows = state.db().config().map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })?;
    let Some(row) = rows.iter().find(|row| row.key == REGISTRY_KEY) else {
        return Err(synra_ipc::IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "why": format!("no registry is configured; set `{REGISTRY_KEY}` to its base URL first"),
        })));
    };
    let url: String = serde_json::from_str(&row.value).map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"field": REGISTRY_KEY, "reason": error.to_string()}))
    })?;
    split_url(url.trim())
}

/// Splits a base URL into its scheme, authority and host.
fn split_url(url: &str) -> Result<Base, synra_ipc::IpcError> {
    let refused = |reason: &str| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"field": REGISTRY_KEY, "reason": reason}))
    };
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    }
    else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    }
    else {
        return Err(refused("it must start with https:// or http://"));
    };
    let authority = rest.split('/').next().unwrap_or_default().trim_end_matches('/');
    if authority.is_empty() {
        return Err(refused("it names no host"));
    }
    // A port is allowed; a path is not, because the paths below are the protocol's.
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|character| character.is_ascii_digit()) => (host, Some(port)),
        _ => (authority, None),
    };
    let host = host.trim_matches(['[', ']']).to_owned();
    if host.is_empty() {
        return Err(refused("it names no host"));
    }
    let origin = match port {
        Some(port) => format!("{}://{}:{port}", if secure { "https" } else { "http" }, host),
        None => format!("{}://{}", if secure { "https" } else { "http" }, host),
    };
    Ok(Base { origin, host, secure })
}

/// The guarded HTTP client for one registry.
fn client(base: &Base) -> Client {
    let mut guard = NetworkGuard::for_hosts(std::slice::from_ref(&base.host));
    if !base.secure {
        // Plain http is only for a loopback registry — a local mirror, a test server. A public host must be https, which
        // is the guard's default rather than a rule added here.
        guard = guard.allowing_insecure(true).with_policy(IpPolicy {
            allow_loopback: true,
            ..IpPolicy::default()
        });
    }
    Client::new(
        guard,
        FetchLimits {
            max_response_bytes: MAX_DOWNLOAD_BYTES,
            ..FetchLimits::default()
        },
    )
}

/// The packument for one package.
///
/// # Errors
///
/// `Unsupported` when no registry is configured, `InvalidArgument` when the registry does not serve the package or serves
/// something that is not a packument, and a fetch refusal's own code for a network problem.
pub fn packument(state: &AppState, plugin_id: &str) -> Result<Packument, synra_ipc::IpcError> {
    let base = base(state)?;
    let url = format!("{}/{}", base.origin, plugin_id.replace('/', "%2F"));
    let response = client(&base)
        .fetch(&get(&url))
        .map_err(|refusal| {
            synra_ipc::IpcError::unknown(refusal.code).with_params(serde_json::json!({
                "url": url,
                "reason": refusal.message,
            }))
        })?;
    if !(200..300).contains(&response.status) {
        return Err(synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
            "pluginId": plugin_id,
            "status": response.status,
            "reason": "the registry does not serve that package",
        })));
    }
    Packument::parse(&response.body).map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"pluginId": plugin_id, "reason": error.to_string()}))
    })
}

/// A tarball extracted under the cache, and the files it wrote so they can be removed without a recursive delete.
struct PackageDir {
    directory: std::path::PathBuf,
    files: Vec<std::path::PathBuf>,
}

impl PackageDir {
    fn path(&self) -> &std::path::Path {
        &self.directory
    }

    /// Removes the files, then the directories that are now empty, then the root.
    fn cleanup(self) {
        for file in self.files.iter().rev() {
            let _ = std::fs::remove_file(file);
        }
        let mut parents: Vec<std::path::PathBuf> = self
            .files
            .iter()
            .filter_map(|file| file.parent().map(std::path::Path::to_path_buf))
            .collect();
        parents.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        parents.dedup();
        for parent in parents {
            if parent.starts_with(&self.directory) {
                let _ = std::fs::remove_dir(parent);
            }
        }
        let _ = std::fs::remove_dir(&self.directory);
    }
}

/// Downloads the chosen version's tarball and opens it into a fresh cache directory.
fn download(
    state: &AppState,
    base: &Base,
    chosen: &synra_registry::Version,
) -> Result<PackageDir, synra_ipc::IpcError> {
    let tarball = &chosen.dist.tarball;
    // ★ Same origin only: a packument that points a package at another host is asking for a fetch nobody configured, and
    // following it would make the registry's answer a capability.
    if !tarball.starts_with(&base.origin) {
        return Err(synra_ipc::IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "tarball": tarball,
            "reason": format!("the tarball is not on the configured registry ({})", base.origin),
        })));
    }
    let response = client(base).fetch_bytes(&get(tarball)).map_err(|refusal| {
        synra_ipc::IpcError::unknown(refusal.code).with_params(serde_json::json!({
            "url": tarball,
            "reason": refusal.message,
        }))
    })?;
    if !(200..300).contains(&response.status) {
        return Err(synra_ipc::IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({
            "url": tarball,
            "status": response.status,
            "reason": "the tarball could not be downloaded",
        })));
    }

    let directory = state
        .paths
        .cache_dir()
        .join("registry")
        .join(uuid::Uuid::new_v4().to_string());
    let files = extract_tgz(
        &response.body,
        &directory,
        Some("package"),
        ArchiveLimits::default(),
    )
    .map_err(|error| {
        // The extractor removes what it wrote; this removes the now-empty root it created.
        let _ = std::fs::remove_dir(&directory);
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
            "tarball": tarball,
            "reason": error.to_string(),
        }))
    })?;
    Ok(PackageDir { directory, files })
}

/// Installs the version a registry offers, through the same pipeline a directory install uses.
///
/// # Errors
///
/// As [`packument`], plus the archive and install refusals, each naming its stage.
pub fn install(
    state: &AppState,
    plugin_id: &str,
    version: Option<&str>,
) -> Result<InstallReport, synra_ipc::IpcError> {
    let base = base(state)?;
    let document = packument(state, plugin_id)?;
    let chosen = match version {
        Some(version) => document.at(version).ok_or_else(|| {
            synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "pluginId": plugin_id,
                "version": version,
                "reason": "the registry does not publish that version",
            }))
        })?,
        None => document.latest().ok_or_else(|| {
            synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "pluginId": plugin_id,
                "reason": "the registry has no `latest` tag for it",
            }))
        })?,
    };

    let package = download(state, &base, chosen)?;
    let outcome = crate::commands::plugins::install_directory(state, package.path());
    package.cleanup();
    outcome
}

/// A `GET` request with no body.
fn get(url: &str) -> Request {
    Request {
        url: url.to_owned(),
        method: "GET".to_owned(),
        headers: BTreeMap::new(),
        body: None,
    }
}

/// The registry branch of [`crate::commands::plugins::plugins_install`].
pub fn install_source(
    state: &AppState,
    plugin_id: &synra_protocol::PluginId,
    version: Option<&str>,
) -> Result<InstallReport, synra_ipc::IpcError> {
    install(state, plugin_id.as_str(), version)
}

/// The `InstallSourceDto` shape, for a caller that only has the source.
pub fn install_from_source(
    state: &AppState,
    source: &InstallSourceDto,
) -> Result<InstallReport, synra_ipc::IpcError> {
    let InstallSourceDto::Registry { plugin_id, version } = source else {
        return Err(synra_ipc::IpcError::unknown(ErrorCode::Unsupported)
            .with_params(serde_json::json!({"why": "not a registry source"})));
    };
    install_source(state, plugin_id, version.as_deref())
}
