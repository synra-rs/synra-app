use super::*;

/// Resolves a static index's tarball reference against the document's own URL.
///
/// Absolute URLs pass through; a path is joined to the index's directory, and a leading `/` to its origin. `..` is
/// refused rather than normalised: an index that climbs out of its own directory is asking to fetch something the author
/// did not mean to publish.
pub(crate) fn resolve_tarball(index_url: &str, tarball: &str) -> Result<String, String> {
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
/// Downloads the chosen version's tarball and opens it into a fresh cache directory.
pub(crate) fn download(
    state: &AppState,
    bases: &[Base],
    source: &Base,
    chosen: &synra_registry::Version,
    index_url: Option<&str>,
) -> Result<synra_core::install::UnpackedTarball, synra_ipc::IpcError> {
    let mut candidates = Vec::new();
    if source.kind == Kind::Static {
        // ★ A static index may name a tarball relative to itself: the author who uploads files usually does not know the
        // bucket's public URL, and a relative name is resolved against the document that carried it. An absolute URL is
        // still allowed — the bytes may be on a CDN — and is checked for scheme and host like any registry.
        let tarball = match index_url {
            Some(index_url) => {
                resolve_tarball(index_url, &chosen.dist.tarball).map_err(|reason| {
                    synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                        serde_json::json!({
                            "tarball": chosen.dist.tarball,
                            "reason": reason,
                        }),
                    )
                })?
            }
            None => chosen.dist.tarball.clone(),
        };
        let base = split_url(&tarball, Kind::Static).map_err(|_| {
            synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "tarball": tarball,
                "reason": "the static index names a tarball that is not an https:// or http:// URL",
            }))
        })?;
        candidates.push((base, tarball));
    } else {
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
                last = Some((
                    ErrorCode::Unsupported,
                    format!("{url} answered {}", response.status),
                ));
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
        let (code, reason) = last.unwrap_or((
            ErrorCode::Internal,
            "no mirror served the tarball".to_owned(),
        ));
        return Err(synra_ipc::IpcError::unknown(code)
            .with_params(serde_json::json!({"tarball": chosen.dist.tarball, "reason": reason})));
    };
    // ★ Verify and unpack are the kernel's, not this host's: the digest is checked before a single entry is written,
    // by the same function the daemon's registry install calls. A document with no integrity string is refused rather
    // than trusted — there is nothing to compare the bytes against.
    let staging = state.paths.cache_dir().join("registry");
    synra_core::install::unpack_published_tarball(
        &bytes,
        chosen.dist.integrity.as_deref(),
        &chosen.dist.tarball,
        &staging,
        synra_store::sqlite::now_ms(),
    )
    .map_err(|refusal| {
        synra_ipc::IpcError::unknown(refusal.code()).with_params(serde_json::json!({
            "tarball": chosen.dist.tarball,
            "reason": format!("{}: {}", refusal.stage, refusal.reason),
        }))
    })
}
