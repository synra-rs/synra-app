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
    if source.kind() == Kind::Static {
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
        candidates.push((artifact_client(source, &tarball)?, tarball));
    } else {
        // ★ An npm tarball is fetched from the mirror that served the packument, or from another **configured** mirror by
        // the same path. A packument that points at an origin nobody configured is asking for a fetch this device never
        // agreed to.
        let tarball = &chosen.dist.tarball;
        let path = source.npm_artifact_path(tarball).map_err(|reason| {
            let code = if reason == synra_registry::ArtifactError::OriginMismatch {
                ErrorCode::HostNotAllowed
            } else {
                ErrorCode::InvalidArgument
            };
            synra_ipc::IpcError::unknown(code)
                .with_params(serde_json::json!({"reason": reason.to_string()}))
        })?;
        candidates.push((client(source), format!("{}{path}", source.url())));
        for other in bases {
            if other.kind() == Kind::Npm && other.url() != source.url() {
                candidates.push((client(other), format!("{}{path}", other.url())));
            }
        }
    }

    let mut last = None;
    let mut body: Option<Vec<u8>> = None;
    for (client, url) in &candidates {
        match client.fetch_bytes(&get(url)) {
            Ok(response) if (200..300).contains(&response.status) => {
                body = Some(response.body);
                break;
            }
            Ok(response) if retryable(response.status) => {
                last = Some(http_failure(response.status, url, Operation::Download));
            }
            Ok(response) => {
                return Err(http_failure(response.status, url, Operation::Download));
            }
            Err(refusal) => {
                last = Some(network_failure(&refusal, url));
            }
        }
    }
    let Some(bytes) = body else {
        return Err(exhausted(last, Operation::Download));
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
