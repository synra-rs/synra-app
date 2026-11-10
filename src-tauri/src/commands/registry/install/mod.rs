use super::*;

mod download;

#[allow(unused_imports)]
pub(crate) use download::{download, resolve_tarball};

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
