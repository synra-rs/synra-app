use super::*;

/// Asks the registry whether a newer version is published for an installed plugin.
///
/// # Errors
///
/// `PluginNotInstalled` when no active version exists, `Unsupported` when no registry is configured, and a fetch refusal's
/// own code for a network problem.
pub fn plugins_check_update(
    state: &AppState,
    plugin_id: &str,
) -> Result<Option<UpdateInfo>, IpcError> {
    if let Some(source) = local_directory_source(state, plugin_id) {
        return check_local_update(state, plugin_id, &source, synra_store::sqlite::now_ms());
    }
    crate::commands::registry::check_update(state, plugin_id)
}

/// Searches the configured registry for plugins.
///
/// The catalogue is a search over the registry's own index, scoped to packages that declare the plugin keyword; the
/// preview still shows the manifest's permissions before an install.
///
/// # Errors
///
/// `Unsupported` when no registry is configured or it serves no search endpoint, and a fetch refusal's own code for a
/// network problem.
pub fn plugins_catalog_search(
    state: &AppState,
    query: &CatalogQuery,
) -> Result<Vec<CatalogItem>, IpcError> {
    crate::commands::registry::search(state, query)
}

/// Describes a registry package before any of it is downloaded.
///
/// ★ A preview is the publisher's claim, read from the packument's inline manifest; the signature check happens on the
/// bytes. It is a question — "these are the permissions, do you want to fetch it?" — and never an approval.
///
/// # Errors
///
/// `Unsupported` when no registry is configured, `InvalidArgument` when the registry does not serve the package or does not
/// publish its manifest inline, and a fetch refusal's own code for a network problem.
pub fn plugins_preview(
    state: &AppState,
    plugin_id: &str,
    version: Option<&str>,
) -> Result<InstallPreviewDto, IpcError> {
    crate::commands::registry::preview(state, plugin_id, version)
}

/// Replaces an installed plugin with the newest version the registry publishes.
///
/// # Returns
///
/// The install report, so the caller can read the version that landed and any warning the checks produced. It used to
/// discard both: the data existed in the pipeline's answer and the command returned nothing.
///
/// # Errors
///
/// `PluginNotInstalled` when no active version exists, `Unsupported` when no registry is configured, and the install
/// pipeline's refusals when the package cannot be fetched or verified.
pub fn plugins_update(state: &AppState, plugin_id: &str) -> Result<InstallReport, IpcError> {
    if let Some(source) = local_directory_source(state, plugin_id) {
        let report = update_directory(state, plugin_id, &source)?;
        announce_updated(state, &report);
        return Ok(report);
    }
    let report = crate::commands::registry::update(state, plugin_id)?;
    // The stored check belongs to the old version and is no longer returned after the installer changes the active row.
    announce_updated(state, &report);
    Ok(report)
}

/// The updates the last pass found, for a page that was not open when it ran.
///
/// # Errors
///
/// `Internal` when a saved check cannot be read.
pub fn plugins_updates(state: &AppState) -> Result<Vec<UpdateInfo>, IpcError> {
    state
        .db()
        .plugin_update_checks()
        .map_err(internal)?
        .iter()
        .filter_map(crate::commands::plugin_updates::info)
        .collect()
}

/// Publishes the page-level "updated" notice, the same one the file-association path sends.
///
/// # Why the command does this as well as returning the report
///
/// Because the caller that updated from the plugin page does not currently render the report, and a warning about an
/// unsigned package must not depend on which button started the update. The notice is the channel every update already
/// has; the report is there for a caller that wants the detail without listening.
pub(crate) fn announce_updated(state: &AppState, report: &InstallReport) {
    state.publish(synra_ipc::event::KernelEvent::Notice {
        message: "notice.updated".to_owned(),
        params: serde_json::json!({
            "pluginId": report.plugin_id.to_string(),
            "version": report.version,
            "warnings": report.warnings,
        }),
    });
}
