use super::*;

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
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(
            serde_json::json!({"pluginId": manifest.id.to_string(), "reason": error.to_string()}),
        )
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
        capabilities: manifest
            .capabilities
            .iter()
            .map(|declaration| declaration.cap.clone())
            .collect(),
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

    let package = download(
        state,
        &bases,
        &registry.base,
        chosen,
        registry.index_url.as_deref(),
    )?;
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
pub(crate) fn publisher_mismatch(
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
/// # Returns
///
/// The install report the update produced. It used to be discarded here and the command returned nothing, so a caller
/// could not tell an update that carried warnings (an unsigned package, a changed publisher) from a clean one — the data
/// existed one line above the `()`. The file-association path already publishes a notice from the same report; this is the
/// registry path's half.
///
/// # Errors
///
/// As [`install`], plus `update` when the plugin is not installed.
pub fn update(state: &AppState, plugin_id: &str) -> Result<InstallReport, synra_ipc::IpcError> {
    let bases = bases(state)?;
    let registry = packument_with_base(state, plugin_id)?;
    let chosen = registry.packument.latest().ok_or_else(|| {
        synra_ipc::IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
            "pluginId": plugin_id,
            "reason": "the registry has no `latest` tag for it",
        }))
    })?;
    let package = download(
        state,
        &bases,
        &registry.base,
        chosen,
        registry.index_url.as_deref(),
    )?;
    let outcome = crate::commands::plugins::update_directory(state, plugin_id, package.path());
    package.cleanup();
    outcome
}

/// Downloads the exact version checked, then lets the kernel admit its verified bytes under the selected policy.
pub(crate) fn update_automatically(
    state: &AppState,
    plugin_id: &str,
    version: &str,
    policy: synra_plugin_api::update::UpdatePolicy,
    cancel: &synra_core::supervisor::Cancellation,
) -> Result<InstallReport, synra_ipc::IpcError> {
    let expected_registry =
        synra_core::plugin_updates::registry_setting(&state.db()).map_err(|error| {
            synra_ipc::IpcError::unknown(ErrorCode::Internal)
                .with_params(serde_json::json!({"detail": error.to_string()}))
        })?;
    let bases = bases(state)?;
    let registry = packument_with_base(state, plugin_id)?;
    let chosen = choose(&registry.packument, plugin_id, Some(version))?;
    let package = download(
        state,
        &bases,
        &registry.base,
        chosen,
        registry.index_url.as_deref(),
    )?;
    let outcome = crate::commands::plugin_updates::apply_automatic_update(
        state,
        plugin_id,
        version,
        package.path(),
        expected_registry.as_deref(),
        policy,
        cancel,
    );
    package.cleanup();
    outcome
}

/// A `GET` request with no body.
pub(crate) fn get(url: &str) -> Request {
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
