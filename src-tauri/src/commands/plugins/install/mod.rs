use super::*;

/// Installs a plugin from a directory or the configured registry, through the checks, and reports what the user has to be
/// told.
///
/// # What an install is not
///
/// It is not an approval. The row this writes is installed, switched off and untrusted, and the answer says `enabled:
/// false` so that a caller cannot read the absence of a decision as a decision.
///
/// # Errors
///
/// * `Unsupported` when this shell has no plugin engine at all, or no registry is configured for a registry source.
/// * `InvalidArgument` when a check refused the package; the params carry the stage and the reason.
pub fn plugins_install(
    state: &AppState,
    source: &InstallSourceDto,
) -> Result<InstallReport, IpcError> {
    match source {
        InstallSourceDto::Path { path } => install_directory(state, std::path::Path::new(path)),
        InstallSourceDto::Registry { .. } => {
            crate::commands::registry::install_from_source(state, source)
        }
        // A source a newer build added is not one this build can fetch; refusing it by name is the same answer the
        // variant got before there was a fetcher.
        _ => Err(
            IpcError::unknown(synra_protocol::ErrorCode::Unsupported).with_params(
                serde_json::json!({"why": "this build does not know that install source"}),
            ),
        ),
    }
}

/// Installs a package from a directory already on disk.
///
/// # Errors
///
/// * `Unsupported` when this shell has no plugin engine at all.
/// * `InvalidArgument` when a check refused the package; the params carry the stage and the reason.
pub(crate) fn install_directory(
    state: &AppState,
    path: &std::path::Path,
) -> Result<InstallReport, IpcError> {
    report_of(run_install(state, path, false)?)
}

/// The report an install or an update gives the caller.
///
/// One function rather than the same six fields at two call sites: an update's warnings are as much a fact as an
/// install's, and the version that landed is read from the same place in both.
fn report_of(installed: synra_core::install::Installed) -> Result<InstallReport, IpcError> {
    Ok(InstallReport {
        plugin_id: parse(&installed.plugin_id)?,
        version: installed.version,
        warnings: installed.warnings,
        enabled: false,
        publisher_id: installed
            .signed_by
            .as_ref()
            .map(|publisher| publisher.publisher_id.clone()),
        fingerprint: installed
            .signed_by
            .as_ref()
            .map(|publisher| publisher.fingerprint.clone()),
    })
}

/// Replaces the installed version of a plugin with the package at `path`.
///
/// # Errors
///
/// As [`install_directory`], plus `update` when nothing is installed at that identifier.
pub(crate) fn update_directory(
    state: &AppState,
    plugin_id: &str,
    path: &std::path::Path,
) -> Result<InstallReport, IpcError> {
    let id = parse(plugin_id)?;
    migrate_and_unload(state, &id, source_version(path).as_deref())?;
    complete_update(state, &id, run_install(state, path, true))
}

/// Migrates the old instance before replacing its files, then stops it.
pub(crate) fn migrate_and_unload(
    state: &AppState,
    id: &PluginId,
    new_version: Option<&str>,
) -> Result<(), IpcError> {
    // ★ The old instance is asked to migrate **before** its bytes are replaced. Only the old code knows what it wrote,
    // and a migration failure has to leave a working version installed — after the replacement there would be nothing
    // to fall back to.
    let old_version = state
        .plugin_set()
        .ok()
        .and_then(|set| set.lock().version(id).map(str::to_owned));
    if let Some(old) = old_version {
        if let Some(new) = new_version
            && new != old
        {
            let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or(0);
            let plugins = state.plugin_set()?;
            let mut set = plugins.lock();
            synra_core::plugin::migrate(&mut set, id, &old, new, now).map_err(|reason| {
                IpcError::unknown(synra_protocol::ErrorCode::PluginTrapped)
                    .with_params(serde_json::json!({"pluginId": id, "reason": reason}))
            })?;
        }
        // The old instance is stopped before its files change: a compiled plugin still holding the previous artifact
        // would answer from bytes the update is about to replace.
        apply_unload(state, id);
    }
    Ok(())
}

/// Starts the replacement or restores the old instance when the commit failed, then publishes its card.
pub(crate) fn complete_update(
    state: &AppState,
    id: &PluginId,
    outcome: Result<synra_core::install::Installed, IpcError>,
) -> Result<InstallReport, IpcError> {
    let installed = match outcome {
        Ok(installed) => installed,
        Err(error) => {
            // The update did not happen, so the old version is put back. It was unloaded above; reloading is the same
            // call an enable makes, and a failure here is already carried by the original error.
            let _ = apply_lifecycle(state, id);
            announce(state, id);
            return Err(error);
        }
    };
    let id = parse(&installed.plugin_id)?;
    // ★ A refusal here is not a failed update. The new version is on disk and its row is written; what failed is
    // starting it, most often because the new manifest changed and the user has to confirm its capabilities again.
    // Returning an error would tell the user the update did not happen, so the sentence goes on the card instead —
    // the same division the consent command uses.
    let _ = apply_lifecycle(state, &id);
    announce(state, &id);
    report_of(installed)
}

/// The version a package declares, whether it is a directory or a `.synra` archive.
///
/// A directory's manifest is read in place; an archive is unpacked into a temporary directory by
/// `synra_core::install::package_manifest`, which throws the files away and keeps the manifest. Without that, an update
/// from a `.synra` file skipped the migration — the one step that has to run against the old instance before its bytes
/// are replaced.
fn source_version(path: &std::path::Path) -> Option<String> {
    if path.is_file() {
        return synra_core::install::package_manifest(path)
            .ok()
            .map(|manifest| manifest.version);
    }
    let text = std::fs::read_to_string(path.join("manifest.json")).ok()?;
    let manifest = synra_plugin_api::Manifest::from_json(&text).ok()?;
    Some(manifest.version)
}

/// Reads the original local directory recorded by the installer, when this plugin came from one.
///
/// Registry installs also carry a source note, but it points at a downloaded archive rather than a directory. Keeping
/// this branch directory-only lets registry packages continue through their configured registry while local development
/// packages compare against the files the author is editing.
pub(crate) fn local_directory_source(
    state: &AppState,
    plugin_id: &str,
) -> Option<std::path::PathBuf> {
    let registry_configured = state
        .db()
        .config()
        .ok()?
        .into_iter()
        .find(|row| row.key == crate::commands::registry::REGISTRY_KEY)
        .and_then(|row| serde_json::from_str::<String>(&row.value).ok())
        .is_some_and(|value| !value.trim().is_empty());
    if registry_configured {
        return None;
    }
    let note = state.paths.plugin_dir(plugin_id).join("source.json");
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(note).ok()?).ok()?;
    let path = std::path::PathBuf::from(value.get("path")?.as_str()?);
    path.is_dir().then_some(path)
}

fn manifest_at(path: &std::path::Path) -> Option<Manifest> {
    if path.is_file() {
        return synra_core::install::package_manifest(path).ok();
    }
    let text = std::fs::read_to_string(path.join("manifest.json")).ok()?;
    Manifest::from_json(&text).ok()
}

fn installed_manifest(state: &AppState, plugin_id: &str) -> Option<Manifest> {
    manifest_at(&state.paths.plugin_dir(plugin_id))
}

pub(crate) fn check_local_update(
    state: &AppState,
    plugin_id: &str,
    source: &std::path::Path,
    now_ms: i64,
) -> Result<Option<UpdateInfo>, IpcError> {
    let id = parse(plugin_id)?;
    let active = state
        .db()
        .active_plugin(plugin_id)
        .map_err(internal)?
        .ok_or_else(|| {
            IpcError::unknown(synra_protocol::ErrorCode::PluginNotInstalled)
                .with_params(serde_json::json!({"pluginId": plugin_id}))
        })?;
    let candidate = manifest_at(source).ok_or_else(|| {
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument).with_params(
            serde_json::json!({
                "pluginId": plugin_id,
                "reason": "the recorded local plugin directory has no readable manifest",
            }),
        )
    })?;
    let available = synra_registry::is_newer(&candidate.version, &active.version);
    let added_capabilities = if available {
        let installed = installed_manifest(state, plugin_id)
            .map(|manifest| {
                manifest
                    .capabilities
                    .into_iter()
                    .map(|capability| capability.cap)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        candidate
            .capabilities
            .iter()
            .map(|capability| capability.cap.clone())
            .filter(|capability| !installed.contains(capability))
            .collect()
    } else {
        Vec::new()
    };
    crate::commands::plugin_updates::record(
        state,
        &PluginUpdateCheck {
            plugin_id: id.to_string(),
            current_version: active.version,
            checked_at_ms: now_ms,
            registry: None,
            available_version: available.then_some(candidate.version),
            added_capabilities,
            attempts: 0,
            retry_at_ms: None,
        },
    )
}

/// The pipeline both install and update run: verify, stage, move, record.
mod pipeline;
pub use pipeline::plugins_uninstall;
pub(crate) use pipeline::run_install;
