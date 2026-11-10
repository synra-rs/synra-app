use super::*;

pub(crate) fn run_install(
    state: &AppState,
    path: &std::path::Path,
    replace: bool,
) -> Result<synra_core::install::Installed, IpcError> {
    let plugins = state.plugin_set()?;
    let set = plugins.lock();
    // The database guard is scoped to the call rather than held while the report is built: this mutex is not reentrant,
    // and a `plugins_list` inside the same breadth would be a hang rather than an error.
    let outcome = {
        let mut db = state.db();
        let now = synra_store::sqlite::now_ms();
        let facts = synra_plugin_host::verify::HostFacts::desktop();
        // ★ A path that is a file is a `.synra` package; a path that is a directory is the unpacked form. Both are the
        // product's own containers, and the interface's "install a package" flow hands over whichever the user picked.
        match (replace, path.is_file()) {
            (false, false) => synra_core::install::from_directory(
                &mut db,
                set.backend(),
                &state.paths.plugins_dir(),
                path,
                now,
                facts,
            ),
            (false, true) => synra_core::install::from_package(
                &mut db,
                set.backend(),
                &state.paths.plugins_dir(),
                path,
                now,
                facts,
            ),
            (true, false) => synra_core::install::update_from_directory(
                &mut db,
                set.backend(),
                &state.paths.plugins_dir(),
                path,
                now,
                facts,
            ),
            (true, true) => synra_core::install::update_from_package(
                &mut db,
                set.backend(),
                &state.paths.plugins_dir(),
                path,
                now,
                facts,
            ),
        }
    };
    let installed = outcome.map_err(|refusal| {
        // ★ The stage the pipeline stopped at, as the label — the refusal already names it, so "installs fail at verify"
        // is a question the recorder can answer without reading a log. A success has one stage, `complete`.
        record_install(state, refusal.stage, "refused");
        IpcError::unknown(refusal.code()).with_params(serde_json::json!({
            "stage": refusal.stage,
            "reason": refusal.reason,
        }))
    })?;
    record_install(state, "complete", "ok");
    // ★ The set's lock is released before anything reads it again.
    //
    // `announce` asks the set what is loaded, and this mutex is not reentrant: holding it across the call was a
    // same-thread self-deadlock, and the symptom was a test that hung rather than one that failed.
    drop(set);
    announce(state, &parse(&installed.plugin_id)?);
    Ok(installed)
}

/// Removes a plugin, and its data when the caller says so.
///
/// # The order, and what a failure in the middle leaves
///
/// The **record** goes first. A directory removed while its row remains is a plugin the list still offers that the
/// loader then reports as broken; the other order leaves files `record_discovered_plugins` would rediscover on the next
/// start, which at least shows the user something true. So the row goes first and the directory second, and a failure of
/// the second refuses the command rather than being swallowed — the user has to know the uninstall is half done.
///
/// # Why there is no event
///
/// Because the only plugin event the interface knows is `PluginUpdated`, which carries a card — and there is no card for
/// a plugin that no longer exists. The page reloads its list after this command, which is the honest shape for a
/// removal.
///
/// # Errors
///
/// `PluginNotInstalled` when no row has that identifier, and `Internal` when the store or the filesystem refuses.
pub fn plugins_uninstall(
    state: &AppState,
    plugin_id: &str,
    remove_data: bool,
) -> Result<RemoveReport, IpcError> {
    let id = parse(plugin_id)?;
    let removed = state
        .db()
        .remove_plugin(&id.to_string())
        .map_err(internal)?;
    if !removed {
        return Err(
            IpcError::unknown(synra_protocol::ErrorCode::PluginNotInstalled)
                .with_params(serde_json::json!({"pluginId": plugin_id})),
        );
    }
    // ★ The compiled form goes with the row. A plugin whose files are gone but whose instance is still in the set can
    // still match a share and run — from an artifact the user just deleted.
    apply_unload(state, &id);
    // ★ The store measures before and after: `bytes_freed` is what is no longer on disk, not what was there before a
    // locked directory survived. The count beside `leftovers` is the number a user reading "freed" is owed.
    let removal = synra_store::PluginFiles::new(state.paths.clone())
        .remove(&id.to_string(), remove_data)
        .map_err(internal)?;
    Ok(RemoveReport {
        bytes_freed: removal.bytes_freed,
        data_kept: !remove_data,
        // ★ What the operating system would not let go. The row is already gone, so the command cannot fail on a locked
        // file — it reports the path. The page says so beside the freed size; a user who is not told goes looking.
        leftovers: removal
            .leftovers
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
    })
}
