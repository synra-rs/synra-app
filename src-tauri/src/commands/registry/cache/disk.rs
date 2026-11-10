use super::*;

/// The on-disk form of a cached document, with the validators that let it be revalidated rather than re-fetched.
///
/// ★ The raw body is stored, not a parsed `Packument`: a format change in the parser then fails at load time (and the file
/// is discarded) instead of silently reading a shape this build no longer means. The version field makes that failure the
/// explicit case rather than a coincidence.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PersistedDocument {
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
pub(crate) const DOCUMENT_CACHE_VERSION: u32 = 1;

/// Where one URL's cached document lives.
pub(crate) fn document_cache_path(state: &AppState, url: &str) -> std::path::PathBuf {
    use sha2::Digest as _;

    let digest = sha2::Sha256::digest(url.as_bytes());
    let name: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    state
        .paths
        .cache_dir()
        .join("registry-documents")
        .join(format!("{name}.json"))
}

/// Reads a cached document back, if it is this build's format, this URL's, and still fresh.
pub(crate) fn load_document(
    state: &AppState,
    url: &str,
    kind: Kind,
    now: i64,
) -> Option<CachedDocument> {
    let path = document_cache_path(state, url);
    let text = std::fs::read_to_string(&path).ok()?;
    let persisted: PersistedDocument = serde_json::from_str(&text).ok()?;
    if persisted.version != DOCUMENT_CACHE_VERSION || persisted.url != url || persisted.kind != kind
    {
        return None;
    }
    if persisted
        .expires_at_ms
        .is_some_and(|expires| expires <= now)
    {
        // Expired: the file is not evidence about the present, and leaving it would grow the cache forever.
        let _ = std::fs::remove_file(&path);
        return None;
    }
    let (document, publishers) = match kind {
        Kind::Npm => (
            Packument::parse(&persisted.body).ok()?,
            std::collections::BTreeMap::new(),
        ),
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
pub(crate) fn store_document(
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
pub(crate) const DOCUMENT_CACHE_TTL_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

/// How many bytes the cached documents may occupy.
///
/// ★ A packument is small and a static index is smaller, so this ceiling is generous for a person with a handful of
/// registries and a wall for a script that asks about thousands. Time alone was not enough: documents with an `ETag` and no
/// `max-age` never expire, and a month is a long time to hold every one of them.
pub(crate) const DOCUMENT_CACHE_MAX_BYTES: u64 = 16 * 1024 * 1024;

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
pub(crate) fn prune_directory(directory: &std::path::Path, now: i64, max_bytes: u64) -> usize {
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
pub(crate) fn max_age_ms(headers: &std::collections::BTreeMap<String, String>) -> Option<i64> {
    headers
        .get("cache-control")?
        .split(',')
        .find_map(|part| part.trim().strip_prefix("max-age="))
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| {
            i64::try_from(seconds)
                .unwrap_or(i64::MAX)
                .saturating_mul(1_000)
        })
}

/// The version a document names, by tag or by string.
///
/// Shared by the preview and the install so the two cannot disagree about which version the user saw.
pub(crate) fn choose<'a>(
    document: &'a Packument,
    plugin_id: &str,
    version: Option<&str>,
) -> Result<&'a Version, synra_ipc::IpcError> {
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
pub(crate) fn published_size(dist: &synra_registry::Dist) -> Option<u64> {
    let size = dist.synra.as_ref()?.get("size")?;
    let wasm = size
        .get("wasm")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let ui = size
        .get("ui")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    Some(wasm.saturating_add(ui))
}
