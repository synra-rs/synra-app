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
use synra_ipc::dto::{
    CatalogItem, CatalogQuery, CatalogSort, InstallPreviewDto, InstallReport, InstallSourceDto, UpdateInfo,
};
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
enum Kind {
    /// An npm-compatible registry: a packument at `{origin}/{id}`.
    Npm,
    /// A static host: one `index.json` per package at `{origin}/{id}.json` (16 §4.4).
    Static,
}

/// A registry address, split into what the guard and the URLs need.
#[derive(Debug, Clone)]
struct Base {
    /// `scheme://authority`, plus the path prefix for a static host, without a trailing slash.
    origin: String,
    /// The host alone, as a capability grants it.
    host: String,
    /// Whether the scheme is `https`.
    secure: bool,
    /// Which document the address serves.
    kind: Kind,
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
        .map(|url| match url.strip_prefix("static:") {
            Some(rest) => split_url(rest.trim(), Kind::Static),
            None => split_url(url, Kind::Npm),
        })
        .collect::<Result<_, _>>()?;
    if parsed.is_empty() {
        return Err(synra_ipc::IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "why": format!("`{REGISTRY_KEY}` names no registry"),
        })));
    }
    Ok(parsed)
}

/// Splits a base URL into its scheme, authority and host.
fn split_url(url: &str, kind: Kind) -> Result<Base, synra_ipc::IpcError> {
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
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return Err(refused("it names no host"));
    }
    let path = rest[authority.len()..].trim_end_matches('/');
    // An npm registry's paths are the protocol's, so a path in its URL would be silently ignored; a static host's prefix
    // is where the bucket's files live, so a path is the point there.
    if kind == Kind::Npm && !path.is_empty() {
        return Err(refused("an npm-compatible registry names only a scheme and a host"));
    }
    // A port is allowed.
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|character| character.is_ascii_digit()) => (host, Some(port)),
        _ => (authority, None),
    };
    let host = host.trim_matches(['[', ']']).to_owned();
    if host.is_empty() {
        return Err(refused("it names no host"));
    }
    let scheme = if secure { "https" } else { "http" };
    let authority_with_port = match port {
        Some(port) => format!("{host}:{port}"),
        None => host.clone(),
    };
    let origin = format!("{scheme}://{authority_with_port}{path}");
    Ok(Base { origin, host, secure, kind })
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
    path_for: impl Fn(&Base) -> String,
    what: &str,
) -> Result<(Base, synra_fetch::Response), synra_ipc::IpcError> {
    let mut last: Option<(ErrorCode, String)> = None;
    for base in bases {
        let path = path_for(base);
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
    let bases: Vec<Base> = bases(state)?.into_iter().filter(|base| base.kind == Kind::Npm).collect();
    if bases.is_empty() {
        // A static index is one document per package; there is no index to search, and a request to one would be a 404
        // dressed up as "nothing matched".
        return Err(synra_ipc::IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "why": "the configured registries serve no search endpoint",
        })));
    }
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
    let (_, response) = get_text(&bases, |_| path.clone(), "the search endpoint")?;
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

/// A parsed registry document, and what a static directory claims about its publishers.
struct RegistryDocument {
    /// Which registry answered, so the tarball can be fetched from it or its mirrors.
    base: Base,
    /// The metadata, in the shape the install reads.
    packument: Packument,
    /// Publisher id to fingerprint, from a static index; empty for an npm registry.
    publishers: std::collections::BTreeMap<String, String>,
    /// The static index's own URL, which a relative tarball is resolved against.
    index_url: Option<String>,
}

/// The packument for one package.
///
/// # Errors
///
/// `Unsupported` when no registry is configured, `InvalidArgument` when the registry does not serve the package or serves
/// something that is not a packument, and a fetch refusal's own code for a network problem.
pub fn packument(state: &AppState, plugin_id: &str) -> Result<Packument, synra_ipc::IpcError> {
    Ok(packument_with_base(state, plugin_id)?.packument)
}

/// Whether the registry publishes a version newer than the one installed, and what it asks for that the old one did not.
///
/// # What is compared, and what is believed
///
/// The published version string and the installed manifest's capabilities — the packument's inline manifest is the
/// publisher's claim about the new version, and this answer is a prompt to look, not a decision: the install still verifies
/// the bytes and the preview still shows the real permissions.
pub fn check_update(
    state: &AppState,
    plugin_id: &str,
) -> Result<Option<UpdateInfo>, synra_ipc::IpcError> {
    let active = state.db().active_plugin(plugin_id).map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"pluginId": plugin_id, "detail": error.to_string()}))
    })?;
    let Some(active) = active else {
        return Err(synra_ipc::IpcError::unknown(ErrorCode::PluginNotInstalled)
            .with_params(serde_json::json!({"pluginId": plugin_id})));
    };
    let document = packument(state, plugin_id)?;
    let Some(latest) = document.latest() else {
        return Ok(None);
    };
    if !synra_registry::is_newer(&latest.version, &active.version) {
        // ★ Not `!=`: a device can hold a version newer than the registry's latest (a rollback, a local build, a mirror
        // that has not caught up), and reporting that as an update would offer a downgrade.
        return Ok(None);
    }
    let installed = installed_capabilities(state, plugin_id);
    let published: Vec<String> = latest
        .synra
        .as_ref()
        .map(|manifest| manifest.capabilities.iter().map(|declaration| declaration.cap.clone()).collect())
        .unwrap_or_default();
    let added: Vec<String> = published
        .into_iter()
        .filter(|capability| !installed.contains(capability))
        .collect();
    Ok(Some(UpdateInfo {
        version: latest.version.clone(),
        needs_new_consent: !added.is_empty(),
        added_capabilities: added,
    }))
}

/// The capabilities in the manifest this device installed, read from the plugin's own directory.
///
/// A missing or unreadable manifest answers an empty list rather than failing the check: an update that cannot be compared
/// is still an update, and refusing to say so would hide the version bump behind a parse error.
fn installed_capabilities(state: &AppState, plugin_id: &str) -> Vec<String> {
    // ★ `plugin_dir`, not a raw join: a scoped id contains a separator, and the directory the installer writes is the
    // sanitised one. Joining the id here looked for `plugins/@acme/hello` while the artifact was at `plugins/acme+hello`,
    // so every update of a scoped plugin was compared against an empty capability set.
    let path = state.paths.plugin_dir(plugin_id).join("manifest.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<synra_plugin_api::Manifest>(&text).ok())
        .map(|manifest| manifest.capabilities.into_iter().map(|declaration| declaration.cap).collect())
        .unwrap_or_default()
}

/// The packument and the mirror that served it.
///
/// The mirror matters because the tarball it names may only be on its origin; the install uses it to decide which other
/// mirrors may serve the same path.
fn packument_with_base(
    state: &AppState,
    plugin_id: &str,
) -> Result<RegistryDocument, synra_ipc::IpcError> {
    let bases = bases(state)?;
    let mut last: Option<(ErrorCode, String)> = None;

    for base in &bases {
        let path = match base.kind {
            Kind::Npm => format!("/{}", plugin_id.replace('/', "%2F")),
            Kind::Static => format!("/{plugin_id}.json"),
        };
        let url = format!("{}{path}", base.origin);

        // ★ The cached document and its validators are cloned before the lock is dropped: a conditional request spends a
        // network round trip, and a mutex held across it would make every other registry read wait for the slowest bucket.
        // An entry whose `max-age` has run out is not sent back as a conditional candidate — the host said how long it
        // was good for, and asking it to revalidate something this side already knows is stale would be a second opinion.
        let now = synra_store::sqlite::now_ms();
        let mut cached = state
            .registry_cache
            .lock()
            .get(&url)
            .filter(|entry| entry.expires_at_ms.is_none_or(|expires| expires > now))
            .cloned();
        // ★ A miss in this process is not a miss on this device: the previous run may have written the document and its
        // validators. Reading it back turns "open the app, check every plugin" from N fetches into N conditional requests.
        if cached.is_none()
            && let Some(loaded) = load_document(state, &url, base.kind, now)
        {
            state.registry_cache.lock().insert(url.clone(), loaded.clone());
            cached = Some(loaded);
        }
        let mut request = get(&url);
        if let Some(entry) = &cached {
            if let Some(etag) = &entry.etag {
                request.headers.insert("if-none-match".to_owned(), etag.clone());
            }
            if let Some(modified) = &entry.last_modified {
                request.headers.insert("if-modified-since".to_owned(), modified.clone());
            }
        }

        match client(base).fetch(&request) {
            Ok(response) if response.status == 304 => {
                // A 304 says "what you have is current". Without a cached document there is nothing to have, so a host
                // that answers one anyway is refused rather than guessed about.
                if let Some(mut entry) = cached {
                    if let Some(age) = max_age_ms(&response.headers) {
                        entry.expires_at_ms = Some(now.saturating_add(age));
                        state.registry_cache.lock().insert(url.clone(), entry.clone());
                    }
                    return Ok(RegistryDocument {
                        base: base.clone(),
                        packument: entry.document,
                        publishers: entry.publishers,
                        index_url: Some(url.clone()),
                    });
                }
                last = Some((
                    ErrorCode::ProtocolViolation,
                    format!("{url} answered 304 without a cached document"),
                ));
            }
            Ok(response) if (200..300).contains(&response.status) => {
                let (document, publishers) = match base.kind {
                    Kind::Npm => (
                        Packument::parse(&response.body).map_err(|error| {
                            synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                                serde_json::json!({"pluginId": plugin_id, "reason": error.to_string()}),
                            )
                        })?,
                        std::collections::BTreeMap::new(),
                    ),
                    Kind::Static => {
                        let index = synra_registry::parse_static_index(&response.body).map_err(|error| {
                            synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                                serde_json::json!({"pluginId": plugin_id, "reason": error.to_string()}),
                            )
                        })?;
                        (index.document, index.publishers)
                    }
                };
                // ★ An npm packument is cached too: the validators are the host's, and a check-update that hits `304`
                // costs one round trip instead of re-parsing a document that has not changed. `publishers` is empty for
                // npm, which is the honest value — only a static directory makes that claim.
                let expires_at_ms = max_age_ms(&response.headers).map(|age| now.saturating_add(age));
                state.registry_cache.lock().insert(
                    url.clone(),
                    CachedDocument {
                        etag: response.headers.get("etag").cloned(),
                        last_modified: response.headers.get("last-modified").cloned(),
                        document: document.clone(),
                        publishers: publishers.clone(),
                        expires_at_ms,
                    },
                );
                // ★ The validators are what make the disk copy useful: without them the next process would have to fetch the
                // body again to learn whether it changed.
                store_document(state, &url, base.kind, &response.body, &response.headers, expires_at_ms);
                return Ok(RegistryDocument {
                    base: base.clone(),
                    packument: document,
                    publishers,
                    index_url: (base.kind == Kind::Static).then(|| url.clone()),
                });
            }
            Ok(response) if retryable(response.status) => {
                last = Some((ErrorCode::Unsupported, format!("{url} answered {}", response.status)));
            }
            Ok(response) => {
                return Err(synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                    serde_json::json!({
                        "pluginId": plugin_id,
                        "status": response.status,
                        "reason": "the registry refused the request",
                    }),
                ));
            }
            Err(refusal) => {
                last = Some((refusal.code, format!("{url}: {}", refusal.message)));
            }
        }
    }

    let (code, reason) = last.unwrap_or((ErrorCode::Unsupported, "no registry serves the package".to_owned()));
    Err(synra_ipc::IpcError::unknown(code).with_params(serde_json::json!({
        "pluginId": plugin_id,
        "reason": reason,
        "tried": bases.len(),
    })))
}

/// The on-disk form of a cached document, with the validators that let it be revalidated rather than re-fetched.
///
/// ★ The raw body is stored, not a parsed `Packument`: a format change in the parser then fails at load time (and the file
/// is discarded) instead of silently reading a shape this build no longer means. The version field makes that failure the
/// explicit case rather than a coincidence.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedDocument {
    /// The format version; anything else is ignored.
    version: u32,
    /// The URL it came from, so a hash collision cannot serve the wrong document.
    url: String,
    /// Which document the body is.
    kind: Kind,
    /// The `ETag` the host answered with.
    #[serde(default)]
    etag: Option<String>,
    /// The `Last-Modified` the host answered with.
    #[serde(default)]
    last_modified: Option<String>,
    /// When it should be re-read, when the host said.
    #[serde(default)]
    expires_at_ms: Option<i64>,
    /// The body exactly as it arrived.
    body: String,
}

/// The cache format this build writes and reads.
const DOCUMENT_CACHE_VERSION: u32 = 1;

/// Where one URL's cached document lives.
fn document_cache_path(state: &AppState, url: &str) -> std::path::PathBuf {
    use sha2::Digest as _;

    let digest = sha2::Sha256::digest(url.as_bytes());
    let name: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    state.paths.cache_dir().join("registry-documents").join(format!("{name}.json"))
}

/// Reads a cached document back, if it is this build's format, this URL's, and still fresh.
fn load_document(state: &AppState, url: &str, kind: Kind, now: i64) -> Option<CachedDocument> {
    let path = document_cache_path(state, url);
    let text = std::fs::read_to_string(&path).ok()?;
    let persisted: PersistedDocument = serde_json::from_str(&text).ok()?;
    if persisted.version != DOCUMENT_CACHE_VERSION || persisted.url != url || persisted.kind != kind {
        return None;
    }
    if persisted.expires_at_ms.is_some_and(|expires| expires <= now) {
        // Expired: the file is not evidence about the present, and leaving it would grow the cache forever.
        let _ = std::fs::remove_file(&path);
        return None;
    }
    let (document, publishers) = match kind {
        Kind::Npm => (Packument::parse(&persisted.body).ok()?, std::collections::BTreeMap::new()),
        Kind::Static => {
            let index = synra_registry::parse_static_index(&persisted.body).ok()?;
            (index.document, index.publishers)
        }
    };
    Some(CachedDocument {
        etag: persisted.etag,
        last_modified: persisted.last_modified,
        document,
        publishers,
        expires_at_ms: persisted.expires_at_ms,
    })
}

/// Writes a document to the cache.
///
/// A cache write that fails is a slower next start, not a failed install, so the error is dropped rather than surfaced.
fn store_document(
    state: &AppState,
    url: &str,
    kind: Kind,
    body: &str,
    headers: &std::collections::BTreeMap<String, String>,
    expires_at_ms: Option<i64>,
) {
    let persisted = PersistedDocument {
        version: DOCUMENT_CACHE_VERSION,
        url: url.to_owned(),
        kind,
        etag: headers.get("etag").cloned(),
        last_modified: headers.get("last-modified").cloned(),
        expires_at_ms,
        body: body.to_owned(),
    };
    let Ok(text) = serde_json::to_string(&persisted) else {
        return;
    };
    let path = document_cache_path(state, url);
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    let _ = std::fs::write(&path, text);
}

/// How long a cached document may sit on disk without being read.
///
/// ★ The snapshot's readers are short-lived by design: a device that has not asked about a registry in a month will fetch
/// it once and be right, and the alternative is a directory that grows with every registry this device ever tried.
const DOCUMENT_CACHE_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

/// How many bytes the cached documents may occupy.
///
/// ★ A packument is small and a static index is smaller, so this ceiling is generous for a person with a handful of
/// registries and a wall for a script that asks about thousands. Time alone was not enough: documents with an `ETag` and no
/// `max-age` never expire, and a month is a long time to hold every one of them.
const DOCUMENT_CACHE_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Removes cached documents nobody has read for a month, then the oldest ones until the directory fits its ceiling.
///
/// A failure to prune is a diagnostic, not a reason to refuse to start; the caller ignores the count.
pub fn prune_documents(state: &AppState, now: i64) -> usize {
    prune_directory(
        &state.paths.cache_dir().join("registry-documents"),
        now,
        DOCUMENT_CACHE_MAX_BYTES,
    )
}

/// The prune itself, with the ceiling as an argument so a test can use a small one.
fn prune_directory(directory: &std::path::Path, now: i64, max_bytes: u64) -> usize {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    let cutoff = now.saturating_sub(DOCUMENT_CACHE_TTL_MS);
    let mut removed = 0;
    let mut kept: Vec<(std::path::PathBuf, u64, i64)> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or_default();
        if modified < cutoff {
            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
            continue;
        }
        kept.push((path, meta.len(), modified));
    }

    let mut total: u64 = kept.iter().map(|(_, size, _)| *size).sum();
    if total <= max_bytes {
        return removed;
    }
    // Oldest first: the document a device has not read for longest is the one a refetch costs least to be wrong about.
    kept.sort_by_key(|(_, _, modified)| *modified);
    for (path, size, _) in kept {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
            removed += 1;
        }
    }
    removed
}

/// The lifetime a `Cache-Control` header grants, in milliseconds.
///
/// Only `max-age` is read: `no-store` and `no-cache` are treated as "no lifetime", which means the entry is revalidated on
/// every use rather than trusted — the conservative reading of a directive this client does not otherwise implement.
fn max_age_ms(headers: &std::collections::BTreeMap<String, String>) -> Option<i64> {
    headers
        .get("cache-control")?
        .split(',')
        .find_map(|part| part.trim().strip_prefix("max-age="))
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| i64::try_from(seconds).unwrap_or(i64::MAX).saturating_mul(1_000))
}

/// The version a document names, by tag or by string.
///
/// Shared by the preview and the install so the two cannot disagree about which version the user saw.
fn choose<'a>(document: &'a Packument, plugin_id: &str, version: Option<&str>) -> Result<&'a Version, synra_ipc::IpcError> {
    // ★ Exact, range or `latest`, decided by the packument — one place for both hosts. The previous version compared the
    // request with the map's keys, so `^1.9` was "not published" even when it matched.
    document.select(version).map_err(|error| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
            "pluginId": plugin_id,
            "version": version,
            "reason": error.to_string(),
        }))
    })
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
    let registry = packument_with_base(state, plugin_id)?;
    let chosen = choose(&registry.packument, plugin_id, version)?;
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
        // ★ The directory's claim, when there is one. It is shown beside the signature prompt after install, not treated as
        // verification: the signature is checked on the bytes, and this is only a second statement to compare it with.
        publisher_key: registry.publishers.get(&manifest.publisher.id).cloned(),
        capabilities: manifest.capabilities.iter().map(|declaration| declaration.cap.clone()).collect(),
        headless: manifest.runtime.headless.is_some(),
        has_ui: manifest.runtime.ui.is_some(),
        size_bytes: published_size(&chosen.dist),
    })
}

/// Resolves a static index's tarball reference against the document's own URL.
///
/// Absolute URLs pass through; a path is joined to the index's directory, and a leading `/` to its origin. `..` is
/// refused rather than normalised: an index that climbs out of its own directory is asking to fetch something the author
/// did not mean to publish.
fn resolve_tarball(index_url: &str, tarball: &str) -> Result<String, String> {
    if tarball.starts_with("https://") || tarball.starts_with("http://") {
        return Ok(tarball.to_owned());
    }
    if tarball.is_empty() {
        return Err("the index names an empty tarball path".to_owned());
    }
    if tarball.split('/').any(|segment| segment == "..") {
        return Err("the tarball path must not contain `..`".to_owned());
    }
    if tarball.starts_with('/') {
        let origin = index_url
            .split_once("://")
            .map(|(scheme, rest)| {
                let authority = rest.split('/').next().unwrap_or_default();
                format!("{scheme}://{authority}")
            })
            .ok_or_else(|| "the index URL is not absolute".to_owned())?;
        return Ok(format!("{origin}{tarball}"));
    }
    let directory = index_url
        .rsplit_once('/')
        .map(|(directory, _)| directory)
        .ok_or_else(|| "the index URL is not absolute".to_owned())?;
    Ok(format!("{directory}/{tarball}"))
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
    index_url: Option<&str>,
) -> Result<PackageDir, synra_ipc::IpcError> {
    let mut candidates = Vec::new();
    if source.kind == Kind::Static {
        // ★ A static index may name a tarball relative to itself: the author who uploads files usually does not know the
        // bucket's public URL, and a relative name is resolved against the document that carried it. An absolute URL is
        // still allowed — the bytes may be on a CDN — and is checked for scheme and host like any registry.
        let tarball = match index_url {
            Some(index_url) => resolve_tarball(index_url, &chosen.dist.tarball).map_err(|reason| {
                synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                    "tarball": chosen.dist.tarball,
                    "reason": reason,
                }))
            })?,
            None => chosen.dist.tarball.clone(),
        };
        let base = split_url(&tarball, Kind::Static).map_err(|_| {
            synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "tarball": tarball,
                "reason": "the static index names a tarball that is not an https:// or http:// URL",
            }))
        })?;
        candidates.push((base, tarball));
    }
    else {
        // ★ An npm tarball is fetched from the mirror that served the packument, or from another **configured** mirror by
        // the same path. A packument that points at an origin nobody configured is asking for a fetch this device never
        // agreed to.
        let tarball = &chosen.dist.tarball;
        let path = tarball.strip_prefix(&source.origin).ok_or_else(|| {
            synra_ipc::IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
                "tarball": tarball,
                "reason": format!("the tarball is not on a configured registry ({})", source.origin),
            }))
        })?;
        candidates.push((source.clone(), tarball.clone()));
        for other in bases {
            if other.origin != source.origin {
                candidates.push((other.clone(), format!("{}{}", other.origin, path)));
            }
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
            .with_params(serde_json::json!({"tarball": chosen.dist.tarball, "reason": reason})));
    };
    // ★ The declared digest is checked **before** anything is unpacked. The packument is a claim by the same server that
    // served the bytes; without this comparison a mirror that answered with other bytes would be installed as the
    // published package. A document with no integrity string is refused rather than trusted: there is nothing to compare.
    verify_tarball(chosen.dist.integrity.as_deref(), &chosen.dist.tarball, &bytes)?;

    let directory = state
        .paths
        .cache_dir()
        .join("registry")
        .join(uuid::Uuid::new_v4().to_string());
    let files = extract_tgz(&bytes, &directory, Some("package"), ArchiveLimits::default()).map_err(|error| {
        // The extractor removes what it wrote; this removes the now-empty root it created.
        let _ = std::fs::remove_dir(&directory);
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
            "tarball": chosen.dist.tarball,
            "reason": error.to_string(),
        }))
    })?;
    Ok(PackageDir { directory, files })
}

/// Checks the bytes a mirror served against the digest the document declares.
///
/// # Why a helper rather than an inline check
///
/// Because "what happens when the document carries no digest" is a decision, and the decision has to be testable without a
/// network. A missing digest is not a reason to skip the check — a publisher that cannot state one has not said which
/// bytes are theirs — so it is refused with the same shape as a mismatch.
fn verify_tarball(
    declared: Option<&str>,
    tarball: &str,
    bytes: &[u8],
) -> Result<(), synra_ipc::IpcError> {
    let Some(declared) = declared else {
        return Err(synra_ipc::IpcError::unknown(ErrorCode::Unsupported).with_params(
            serde_json::json!({
                "tarball": tarball,
                "reason": "the registry's entry carries no integrity string, and bytes nothing vouches for are not installed",
            }),
        ));
    };
    synra_plugin_host::registry::check_integrity(tarball, declared, bytes)
        .map(|_| ())
        .map_err(|failure| {
        use synra_plugin_host::registry::FetchFailure;
        let code = match failure {
            FetchFailure::IntegrityMismatch { .. } => ErrorCode::IntegrityMismatch,
            FetchFailure::Network { .. } => ErrorCode::DeviceOffline,
            _ => ErrorCode::Unsupported,
        };
        synra_ipc::IpcError::unknown(code).with_params(serde_json::json!({
            "tarball": tarball,
            "reason": failure.to_string(),
        }))
    })
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
    let registry = packument_with_base(state, plugin_id)?;
    let chosen = choose(&registry.packument, plugin_id, version)?;

    let package = download(state, &bases, &registry.base, chosen, registry.index_url.as_deref())?;
    let outcome = crate::commands::plugins::install_directory(state, package.path());
    package.cleanup();
    let mut report = outcome?;
    if let Some(warning) = publisher_mismatch(
        &registry.publishers,
        report.publisher_id.as_deref(),
        report.fingerprint.as_deref(),
    ) {
        report.warnings.push(warning);
    }
    Ok(report)
}

/// A sentence when a static directory's claimed key disagrees with the fingerprint that signed the package.
///
/// ★ A mismatch is a fact, not a verdict. Both documents are the author's, and either could be the stale one; refusing the
/// install on it would give a typo in `publishers` the power to block a release, while saying nothing would waste the one
/// second opinion the directory offers.
fn publisher_mismatch(
    publishers: &std::collections::BTreeMap<String, String>,
    publisher: Option<&str>,
    fingerprint: Option<&str>,
) -> Option<String> {
    let publisher = publisher?;
    let fingerprint = fingerprint?;
    let claimed = publishers.get(publisher)?;
    (claimed != fingerprint).then(|| {
        format!(
            "the index lists key {claimed} for {publisher}, but the package was signed by {fingerprint}"
        )
    })
}

/// Replaces an installed plugin with the newest version the registry publishes.
///
/// # Errors
///
/// As [`install`], plus `update` when the plugin is not installed.
pub fn update(state: &AppState, plugin_id: &str) -> Result<(), synra_ipc::IpcError> {
    let bases = bases(state)?;
    let registry = packument_with_base(state, plugin_id)?;
    let chosen = registry.packument.latest().ok_or_else(|| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
            "pluginId": plugin_id,
            "reason": "the registry has no `latest` tag for it",
        }))
    })?;
    let package = download(state, &bases, &registry.base, chosen, registry.index_url.as_deref())?;
    let outcome = crate::commands::plugins::update_directory(state, package.path());
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

#[cfg(test)]
mod tests {
    use super::publisher_mismatch;
    use std::collections::BTreeMap;
    use synra_protocol::error::ErrorCode;

    #[test]
    fn a_relative_tarball_is_resolved_against_the_index() {
        assert_eq!(
            super::resolve_tarball("https://bucket.example/plugins/@acme/hello.json", "hello.tgz"),
            Ok("https://bucket.example/plugins/@acme/hello.tgz".to_owned())
        );
        assert_eq!(
            super::resolve_tarball("https://bucket.example/plugins/@acme/hello.json", "/x/hello.tgz"),
            Ok("https://bucket.example/x/hello.tgz".to_owned())
        );
        assert_eq!(
            super::resolve_tarball("https://cdn.example/x.json", "https://other.example/y.tgz"),
            Ok("https://other.example/y.tgz".to_owned())
        );
    }

    #[test]
    fn a_relative_tarball_may_not_climb_out_of_its_directory() {
        // ★ `..` is refused rather than normalised: an index that walks out of its own directory is asking for a fetch the
        // author did not mean to publish.
        assert!(super::resolve_tarball("https://bucket.example/a/hello.json", "../secret.tgz").is_err());
        assert!(super::resolve_tarball("https://bucket.example/a/hello.json", "").is_err());
    }

    #[test]
    fn a_tarball_without_integrity_is_refused() {
        // ★ No digest is not "skip the check": there is nothing to compare the bytes with, and installing them would be
        // trusting whatever answered the connection.
        let error = super::verify_tarball(None, "https://registry.example/x.tgz", b"bytes")
            .expect_err("nothing to verify against");
        assert_eq!(error.code, ErrorCode::Unsupported);
    }

    #[test]
    fn a_tarball_must_match_the_declared_digest() {
        let bytes = b"the published tarball";
        let declared = synra_core::ObjectDigest::of(bytes).as_sri().to_string();
        super::verify_tarball(Some(&declared), "https://registry.example/x.tgz", bytes)
            .expect("the bytes match");

        let error = super::verify_tarball(Some(&declared), "https://registry.example/x.tgz", b"other bytes")
            .expect_err("a mismatch is refused");
        assert_eq!(error.code, ErrorCode::IntegrityMismatch);
    }

    #[test]
    fn a_directory_key_that_matches_is_silent() {
        let mut publishers = BTreeMap::new();
        publishers.insert("@acme".to_owned(), "AB12".to_owned());
        assert_eq!(publisher_mismatch(&publishers, Some("@acme"), Some("AB12")), None);
    }

    #[test]
    fn a_directory_key_that_differs_is_a_sentence() {
        // ★ The sentence names both values: a person can only judge the disagreement if it shows what disagreed.
        let mut publishers = BTreeMap::new();
        publishers.insert("@acme".to_owned(), "AB12".to_owned());

        let warning = publisher_mismatch(&publishers, Some("@acme"), Some("CD34")).expect("a warning");

        assert!(warning.contains("AB12"), "{warning}");
        assert!(warning.contains("CD34"), "{warning}");
        assert!(warning.contains("@acme"), "{warning}");
    }

    #[test]
    fn nothing_to_compare_is_not_a_mismatch() {
        let mut publishers = BTreeMap::new();
        publishers.insert("@acme".to_owned(), "AB12".to_owned());
        assert_eq!(publisher_mismatch(&publishers, Some("@other"), Some("CD34")), None);
        assert_eq!(publisher_mismatch(&publishers, Some("@acme"), None), None);
        assert_eq!(publisher_mismatch(&publishers, None, Some("AB12")), None);
    }

    fn static_document() -> String {
        serde_json::json!({
            "name": "@acme/hello",
            "latest": "1.0.0",
            "publishers": { "@acme": "AB12" },
            "versions": [{ "version": "1.0.0", "tarball": "hello.tgz" }]
        })
        .to_string()
    }

    fn headers() -> BTreeMap<String, String> {
        let mut headers = BTreeMap::new();
        headers.insert("etag".to_owned(), "\"v1\"".to_owned());
        headers
    }

    /// Sets a file's modification time, so a test can make it look old.
    fn set_modified(path: &std::path::Path, at_ms: i64) {
        let file = std::fs::File::options().write(true).open(path).expect("the file");
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_millis(u64::try_from(at_ms).expect("a positive time"));
        file.set_times(std::fs::FileTimes::new().set_modified(time)).expect("an mtime");
    }

    #[test]
    fn a_document_survives_the_process_that_fetched_it() {
        // ★ The point of the disk copy is the next process: validators and body together, so it can revalidate instead of
        // downloading the same document again.
        let (_root, state) = crate::testing::file_state();
        let url = "https://bucket.example/plugins/@acme/hello.json";
        super::store_document(
            &state,
            url,
            super::Kind::Static,
            &static_document(),
            &headers(),
            None,
        );

        let loaded = super::load_document(&state, url, super::Kind::Static, 0).expect("loaded");

        assert_eq!(loaded.etag.as_deref(), Some("\"v1\""));
        assert_eq!(loaded.document.latest().expect("a latest").version, "1.0.0");
        assert_eq!(loaded.publishers.get("@acme").map(String::as_str), Some("AB12"));
    }

    #[test]
    fn an_expired_document_is_removed_rather_than_served() {
        let (_root, state) = crate::testing::file_state();
        let url = "https://bucket.example/plugins/@acme/hello.json";
        super::store_document(
            &state,
            url,
            super::Kind::Static,
            &static_document(),
            &headers(),
            Some(10),
        );

        assert!(super::load_document(&state, url, super::Kind::Static, 11).is_none());
        assert!(!super::document_cache_path(&state, url).exists(), "the stale file is gone");
    }

    #[test]
    fn a_document_nobody_read_for_a_month_is_removed() {
        // ★ A document with only an `ETag` has no expiry, so the load path never deletes it. Without this prune the
        // directory keeps one file per registry this device ever asked about.
        let (_root, state) = crate::testing::file_state();
        let url = "https://bucket.example/plugins/@acme/hello.json";
        super::store_document(&state, url, super::Kind::Static, &static_document(), &headers(), None);
        let path = super::document_cache_path(&state, url);
        let file = std::fs::File::options().write(true).open(&path).expect("the file");
        file.set_times(
            std::fs::FileTimes::new()
                .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 24 * 3600)),
        )
        .expect("an old mtime");
        drop(file);

        let removed = super::prune_documents(&state, synra_store::sqlite::now_ms());

        assert!(removed >= 1);
        assert!(!path.exists());
    }

    #[test]
    fn a_fresh_document_is_kept() {
        let (_root, state) = crate::testing::file_state();
        let url = "https://bucket.example/plugins/@acme/hello.json";
        super::store_document(&state, url, super::Kind::Static, &static_document(), &headers(), None);
        let path = super::document_cache_path(&state, url);

        let removed = super::prune_documents(&state, synra_store::sqlite::now_ms());

        assert_eq!(removed, 0);
        assert!(path.exists());
    }

    #[test]
    fn the_oldest_documents_are_dropped_when_the_cache_is_too_large() {
        // ★ Time alone leaves a script that asks about thousands of registries in one month with a directory of thousands of
        // documents. The oldest go first, and only until the ceiling is met.
        let directory =
            std::env::temp_dir().join(format!("synra-registry-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory");
        let now = synra_store::sqlite::now_ms();
        let oldest = directory.join("oldest.json");
        let newest = directory.join("newest.json");
        std::fs::write(&oldest, b"0123456789").expect("an old file");
        std::fs::write(&newest, b"0123456789").expect("a new file");
        set_modified(&oldest, now - 1_000_000);
        set_modified(&newest, now);

        let removed = super::prune_directory(&directory, now, 10);

        assert_eq!(removed, 1);
        assert!(!oldest.exists(), "the oldest document is the one that went");
        assert!(newest.exists());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_document_of_another_format_is_ignored() {
        // ★ A future format is not this format. Ignoring it (and refetching) is right; parsing it as if it were the same
        // shape is how a cache turns into a bug that looks like a registry problem.
        let (_root, state) = crate::testing::file_state();
        let url = "https://bucket.example/plugins/@acme/hello.json";
        super::store_document(
            &state,
            url,
            super::Kind::Static,
            &static_document(),
            &headers(),
            None,
        );
        let path = super::document_cache_path(&state, url);
        let mut value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("the file")).expect("JSON");
        value["version"] = serde_json::json!(999);
        std::fs::write(&path, value.to_string()).expect("the changed file");

        assert!(super::load_document(&state, url, super::Kind::Static, 0).is_none());
    }
}
