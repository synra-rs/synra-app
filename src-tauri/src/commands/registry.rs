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
use synra_ipc::dto::{CatalogItem, CatalogQuery, CatalogSort, InstallPreviewDto, InstallReport, InstallSourceDto};
use synra_protocol::ErrorCode;
use synra_protocol::PluginId;
use synra_registry::{Limits as ArchiveLimits, Packument, Version, extract_tgz};

use crate::state::AppState;

/// The configuration key holding the registry's base URL.
pub const REGISTRY_KEY: &str = "plugins.registry";

/// How large a response may be when a package is being downloaded.
///
/// The design's package ceiling is 64 MiB (16 §2.1); the client's ceiling is one byte more so an artefact that is exactly
/// the limit is accepted, and the archive's own limits do the rest once it is in memory.
const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024 + 1;

/// A registry address, split into what the guard and the URLs need.
#[derive(Debug, Clone)]
struct Base {
    /// `scheme://authority`, without a trailing slash.
    origin: String,
    /// The host alone, as a capability grants it.
    host: String,
    /// Whether the scheme is `https`.
    secure: bool,
}

/// The configured registries, in the order they are tried.
///
/// The key holds either one URL or an array of mirrors; an array is tried in order, and only for failures that another
/// mirror could plausibly fix (16 §4.2). A verification failure never falls back — it is not a network problem, and
/// retrying it somewhere else would be skipping the check the design exists to keep.
fn bases(state: &AppState) -> Result<Vec<Base>, synra_ipc::IpcError> {
    let rows = state.db().config().map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })?;
    let Some(row) = rows.iter().find(|row| row.key == REGISTRY_KEY) else {
        return Err(synra_ipc::IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "why": format!("no registry is configured; set `{REGISTRY_KEY}` to its base URL first"),
        })));
    };
    let value: serde_json::Value = serde_json::from_str(&row.value).map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"field": REGISTRY_KEY, "reason": error.to_string()}))
    })?;
    let urls: Vec<String> = match value {
        serde_json::Value::String(url) => vec![url],
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => {
            return Err(synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                serde_json::json!({"field": REGISTRY_KEY, "reason": "it must be a URL or an array of URLs"}),
            ));
        }
    };
    let parsed: Vec<Base> = urls
        .iter()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
        .map(split_url)
        .collect::<Result<_, _>>()?;
    if parsed.is_empty() {
        return Err(synra_ipc::IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "why": format!("`{REGISTRY_KEY}` names no registry"),
        })));
    }
    Ok(parsed)
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

/// Whether another mirror could plausibly fix this status.
///
/// ★ 404 and 429 are the design's two (16 §4.2), plus the gateway-shaped 5xx. A 401 or a 403 is a decision the registry
/// made about this client, and asking a second registry the same question would not change it.
fn retryable(status: u16) -> bool {
    matches!(status, 404 | 408 | 429 | 500..=599)
}

/// Fetches one path from the first mirror that answers usefully, returning the mirror and the response.
fn get_text(
    bases: &[Base],
    path: &str,
    what: &str,
) -> Result<(Base, synra_fetch::Response), synra_ipc::IpcError> {
    let mut last: Option<(ErrorCode, String)> = None;
    for base in bases {
        let url = format!("{}{path}", base.origin);
        match client(base).fetch(&get(&url)) {
            Ok(response) if (200..300).contains(&response.status) => return Ok((base.clone(), response)),
            Ok(response) if retryable(response.status) => {
                last = Some((
                    ErrorCode::Unsupported,
                    format!("{url} answered {}", response.status),
                ));
            }
            Ok(response) => {
                return Err(synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                    serde_json::json!({
                        "url": url,
                        "status": response.status,
                        "reason": format!("{what} refused the request"),
                    }),
                ));
            }
            Err(refusal) => {
                last = Some((refusal.code, format!("{url}: {}", refusal.message)));
            }
        }
    }
    let (code, reason) = last.unwrap_or((ErrorCode::Unsupported, format!("no registry serves {what}")));
    Err(synra_ipc::IpcError::unknown(code).with_params(serde_json::json!({
        "reason": reason,
        "tried": bases.len(),
    })))
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

/// What the npm search endpoint answers.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    objects: Vec<SearchObject>,
}

/// One result's envelope.
#[derive(Debug, Deserialize)]
struct SearchObject {
    package: SearchPackage,
}

/// The package fields a result carries.
#[derive(Debug, Deserialize)]
struct SearchPackage {
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    publisher: Option<SearchPublisher>,
}

/// Who published the package, as the search endpoint reports it.
#[derive(Debug, Deserialize)]
struct SearchPublisher {
    #[serde(default)]
    username: Option<String>,
}

/// The most results a caller may receive, whatever it asked for.
const MAX_SEARCH_RESULTS: u32 = 50;

/// How many results a search returns when the caller names no limit.
const DEFAULT_SEARCH_RESULTS: u32 = 20;

/// Searches the configured registry's npm-compatible search endpoint.
///
/// The query is scoped to `keywords:synra-plugin` so an ordinary npm package is not offered as a plugin. Results whose
/// name is not a plugin id are dropped rather than shown as something this device could install.
///
/// # Errors
///
/// `Unsupported` when no registry is configured, and a fetch refusal's own code for a network problem.
pub fn search(state: &AppState, query: &CatalogQuery) -> Result<Vec<CatalogItem>, synra_ipc::IpcError> {
    let bases = bases(state)?;
    let text = query.query.trim();
    let scope = if text.is_empty() {
        "keywords:synra-plugin".to_owned()
    }
    else {
        format!("keywords:synra-plugin {text}")
    };
    let limit = query.limit.unwrap_or(DEFAULT_SEARCH_RESULTS).clamp(1, MAX_SEARCH_RESULTS);
    let offset = query.offset.unwrap_or(0);
    let path = format!("/-/v1/search?text={}&size={limit}&from={offset}", encode(&scope));
    let (_, response) = get_text(&bases, &path, "the search endpoint")?;
    let parsed: SearchResponse = serde_json::from_str(&response.body).map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"reason": error.to_string()}))
    })?;
    let installed: std::collections::BTreeSet<String> = state
        .db()
        .plugins()
        .map(|rows| rows.into_iter().map(|row| row.plugin_id).collect())
        .unwrap_or_default();
    let mut items: Vec<CatalogItem> = parsed
        .objects
        .into_iter()
        .filter_map(|object| {
            let package = object.package;
            let plugin_id = PluginId::try_from(package.name.clone()).ok()?;
            Some(CatalogItem {
                plugin_id,
                title: package.name.clone(),
                description: package.description,
                version: package.version,
                publisher: package.publisher.and_then(|publisher| publisher.username).unwrap_or_default(),
                // The search endpoint does not report a manifest, so the count is unknown here; the preview shows the
                // real list before anything is installed.
                capability_count: 0,
                installed: installed.contains(&package.name),
            })
        })
        .collect();
    // ★ Sorting happens here because npm's index has no `sort` parameter: relevance is whatever order it returned, and
    // name is a comparison this side can make honestly. A sort the response cannot support is not offered (see
    // `CatalogSort`), rather than silently returning relevance under another name.
    if query.sort == Some(CatalogSort::Name) {
        items.sort_by_key(|item| item.title.to_lowercase());
    }
    items.truncate(limit as usize);
    Ok(items)
}

/// Percent-encodes a query string for a URL.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(byte)),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The packument for one package.
///
/// # Errors
///
/// `Unsupported` when no registry is configured, `InvalidArgument` when the registry does not serve the package or serves
/// something that is not a packument, and a fetch refusal's own code for a network problem.
pub fn packument(state: &AppState, plugin_id: &str) -> Result<Packument, synra_ipc::IpcError> {
    Ok(packument_with_base(state, plugin_id)?.1)
}

/// The packument and the mirror that served it.
///
/// The mirror matters because the tarball it names may only be on its origin; the install uses it to decide which other
/// mirrors may serve the same path.
fn packument_with_base(
    state: &AppState,
    plugin_id: &str,
) -> Result<(Base, Packument), synra_ipc::IpcError> {
    let bases = bases(state)?;
    let path = format!("/{}", plugin_id.replace('/', "%2F"));
    let (base, response) = get_text(&bases, &path, "the package")?;
    let document = Packument::parse(&response.body).map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"pluginId": plugin_id, "reason": error.to_string()}))
    })?;
    Ok((base, document))
}

/// The version a document names, by tag or by string.
///
/// Shared by the preview and the install so the two cannot disagree about which version the user saw.
fn choose<'a>(document: &'a Packument, plugin_id: &str, version: Option<&str>) -> Result<&'a Version, synra_ipc::IpcError> {
    match version {
        Some(version) => document.at(version).ok_or_else(|| {
            synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "pluginId": plugin_id,
                "version": version,
                "reason": "the registry does not publish that version",
            }))
        }),
        None => document.latest().ok_or_else(|| {
            synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "pluginId": plugin_id,
                "reason": "the registry has no `latest` tag for it",
            }))
        }),
    }
}

/// What the file a packument points at says about its own size.
///
/// The number is the publisher's claim, in the registry's extension block, and the preview labels it as such; a document
/// without one answers `None` rather than `0`, because "no size" and "empty" are different answers.
fn published_size(dist: &synra_registry::Dist) -> Option<u64> {
    let size = dist.synra.as_ref()?.get("size")?;
    let wasm = size.get("wasm").and_then(serde_json::Value::as_u64).unwrap_or(0);
    let ui = size.get("ui").and_then(serde_json::Value::as_u64).unwrap_or(0);
    Some(wasm.saturating_add(ui))
}

/// Describes a package before any of it is downloaded.
///
/// # Errors
///
/// As [`packument`], plus `InvalidArgument` when the version carries no inline manifest — a preview that showed no
/// permissions would be a confirmation dialog that confirms nothing.
pub fn preview(
    state: &AppState,
    plugin_id: &str,
    version: Option<&str>,
) -> Result<InstallPreviewDto, synra_ipc::IpcError> {
    let document = packument(state, plugin_id)?;
    let chosen = choose(&document, plugin_id, version)?;
    let manifest = chosen.synra.as_ref().ok_or_else(|| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
            "pluginId": plugin_id,
            "version": chosen.version,
            "reason": "the registry does not publish the manifest inline, so the permissions cannot be shown yet",
        }))
    })?;
    let plugin_id = PluginId::try_from(manifest.id.to_string()).map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"pluginId": manifest.id.to_string(), "reason": error.to_string()}))
    })?;
    Ok(InstallPreviewDto {
        plugin_id,
        version: chosen.version.clone(),
        title: manifest.title.clone(),
        description: manifest.description.clone(),
        publisher: manifest.publisher.id.clone(),
        publisher_name: manifest.publisher.name.clone(),
        capabilities: manifest.capabilities.iter().map(|declaration| declaration.cap.clone()).collect(),
        headless: manifest.runtime.headless.is_some(),
        has_ui: manifest.runtime.ui.is_some(),
        size_bytes: published_size(&chosen.dist),
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
    bases: &[Base],
    source: &Base,
    chosen: &synra_registry::Version,
) -> Result<PackageDir, synra_ipc::IpcError> {
    let tarball = &chosen.dist.tarball;
    // ★ A tarball is fetched from the mirror that served the packument, or from another **configured** mirror by the same
    // path. A packument that points at an origin nobody configured is asking for a fetch this device never agreed to.
    let path = tarball.strip_prefix(&source.origin).ok_or_else(|| {
        synra_ipc::IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "tarball": tarball,
            "reason": format!("the tarball is not on a configured registry ({})", source.origin),
        }))
    })?;
    let mut candidates = vec![(source.clone(), tarball.clone())];
    for other in bases {
        if other.origin != source.origin {
            candidates.push((other.clone(), format!("{}{}", other.origin, path)));
        }
    }

    let mut last: Option<(ErrorCode, String)> = None;
    let mut body: Option<Vec<u8>> = None;
    for (base, url) in &candidates {
        match client(base).fetch_bytes(&get(url)) {
            Ok(response) if (200..300).contains(&response.status) => {
                body = Some(response.body);
                break;
            }
            Ok(response) if retryable(response.status) => {
                last = Some((ErrorCode::Unsupported, format!("{url} answered {}", response.status)));
            }
            Ok(response) => {
                return Err(synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                    serde_json::json!({"url": url, "status": response.status, "reason": "the tarball was refused"}),
                ));
            }
            Err(refusal) => {
                last = Some((refusal.code, format!("{url}: {}", refusal.message)));
            }
        }
    }
    let Some(bytes) = body else {
        let (code, reason) = last.unwrap_or((ErrorCode::Internal, "no mirror served the tarball".to_owned()));
        return Err(synra_ipc::IpcError::unknown(code)
            .with_params(serde_json::json!({"tarball": tarball, "reason": reason})));
    };

    let directory = state
        .paths
        .cache_dir()
        .join("registry")
        .join(uuid::Uuid::new_v4().to_string());
    let files = extract_tgz(&bytes, &directory, Some("package"), ArchiveLimits::default()).map_err(|error| {
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
    let bases = bases(state)?;
    let (source, document) = packument_with_base(state, plugin_id)?;
    let chosen = choose(&document, plugin_id, version)?;

    let package = download(state, &bases, &source, chosen)?;
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
