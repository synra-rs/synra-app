use super::*;

mod snapshot;

#[allow(unused_imports)]
pub(crate) use snapshot::{life_state, snapshot};

/// The plugins this device has, as cards.
///
/// # Errors
///
/// `Internal` when the table or a row cannot be read: an empty list would say "nothing is installed", which is a
/// different and false answer.
pub fn plugins_list(state: &AppState) -> Result<Vec<PluginSnapshot>, IpcError> {
    let rows = state.db().plugins().map_err(internal)?;

    // ★ The two plugin gauges the design names, set where the truth is read rather than incremented on every write: a
    // write that failed half-way leaves no counter to correct, and a database edited by a tool is reflected by the next
    // list. `active` is the user's switch, not "loaded in this process" — a faulted plugin is still enabled until the
    // user says otherwise.
    let installed = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    let active = i64::try_from(rows.iter().filter(|row| row.enabled).count()).unwrap_or(i64::MAX);
    state.metrics.gauge_set(
        &synra_observe::metrics::SeriesKey::new("plugins_installed", []),
        installed,
    );
    state.metrics.gauge_set(
        &synra_observe::metrics::SeriesKey::new("plugins_active", []),
        active,
    );

    // ★ Each plugin's sandbox size, as a gauge per plugin. Sampled where the list is read, because the directory grows
    // while the process runs and there is no single write to hook: a plugin's data is whatever it left on disk.
    for row in &rows {
        let bytes = directory_size(&state.paths.plugin_sandbox_dir(&row.plugin_id));
        state.metrics.gauge_set(
            &synra_observe::metrics::SeriesKey::new(
                "plugin_data_bytes",
                [("plugin".to_owned(), row.plugin_id.clone())],
            ),
            i64::try_from(bytes).unwrap_or(i64::MAX),
        );
    }

    // ★ One lock for the whole list.
    //
    // Taking it once per row would make a twenty-plugin list take the lock twenty times, and a plugin that compiles
    // slowly in between would make the list visibly uneven. One snapshot of the set, then every row answers against the
    // same snapshot — so the list is a moment in time rather than twenty.
    let loaded: Option<Vec<PluginId>> = state.plugin_set().ok().map(|set| set.lock().ids());

    let mut snapshots = Vec::with_capacity(rows.len());
    for row in rows {
        let plugin_id = PluginId::try_from(row.plugin_id.clone()).map_err(|error| {
            IpcError::unknown(synra_protocol::ErrorCode::Internal).with_params(
                serde_json::json!({"pluginId": row.plugin_id, "detail": error.to_string()}),
            )
        })?;
        snapshots.push(snapshot(state, &row, &plugin_id, loaded.as_deref())?);
    }
    Ok(snapshots)
}

/// Enables a plugin.
///
/// # Errors
///
/// * [`IpcError::NotFound`] when no plugin of that identifier is installed.
/// * [`IpcError::Internal`] when the change cannot be written.
pub fn plugins_enable(state: &AppState, plugin_id: &str) -> Result<(), IpcError> {
    set_enabled(state, plugin_id, true)
}

/// Disables a plugin.
///
/// # Errors
///
/// As [`plugins_enable`].
pub fn plugins_disable(state: &AppState, plugin_id: &str) -> Result<(), IpcError> {
    set_enabled(state, plugin_id, false)
}

/// Writes the enabled flag.
///
/// # Errors
///
/// [`IpcError::NotFound`] when the plugin is not installed. Read back after the write rather than assumed, because an
/// `UPDATE` that matches no row reports success — and a caller that believed it would show the user a plugin that is
/// still off.
pub(crate) fn set_enabled(
    state: &AppState,
    plugin_id: &str,
    enabled: bool,
) -> Result<(), IpcError> {
    let id = parse(plugin_id)?;
    if !state
        .db()
        .set_plugin_enabled(&id.to_string(), enabled, state.started_at_ms)
        .map_err(internal)?
    {
        return Err(
            IpcError::unknown(synra_protocol::ErrorCode::PluginNotInstalled)
                .with_params(serde_json::json!({"pluginId": plugin_id})),
        );
    }
    // ★ The interface learns about it here rather than on its next snapshot. The frontend has had an event dispatcher
    // since before there was anything to dispatch; a switch that flipped in the store and nowhere else is what that
    // gap looked like from the window.
    //
    // ★ And the set follows the row: enabling loads, disabling unloads. Without this the switch would only take effect
    // at the next start, which is a different product than "enable" promises.
    let lifecycle = apply_lifecycle(state, &id);
    announce(state, &id);
    match lifecycle {
        Ok(()) => Ok(()),
        Err(reason) => Err(IpcError::unknown(synra_protocol::ErrorCode::PluginTrapped)
            .with_params(serde_json::json!({"pluginId": plugin_id, "reason": reason}))),
    }
}

/// Tells the interface that a plugin's card changed, when it can be read back.
///
/// Failure to read the card is not a failure of the command: the write already happened, and refusing the command
/// afterwards would tell the user their change did not take effect when it did — the same reasoning as an audit write
/// that fails after the call it describes.
pub(crate) fn announce(state: &AppState, plugin_id: &PluginId) {
    let Ok(rows) = state.db().plugins() else {
        return;
    };
    let Some(row) = rows
        .into_iter()
        .find(|row| row.plugin_id == plugin_id.to_string())
    else {
        return;
    };
    let loaded: Option<Vec<PluginId>> = state.plugin_set().ok().map(|set| set.lock().ids());
    let Ok(card) = snapshot(state, &row, plugin_id, loaded.as_deref()) else {
        return;
    };
    state.publish(synra_ipc::event::KernelEvent::PluginUpdated(Box::new(card)));
}

/// Reads a plugin's manifest off disk.
///
/// Returns `None` rather than a refusal, because one unreadable manifest among twenty installed plugins must not stop the
/// other nineteen from being listed.
pub(crate) fn read_manifest(state: &AppState, plugin_id: &str) -> Option<Manifest> {
    let text = read_manifest_text(state, plugin_id)?;
    serde_json::from_str(&text).ok()
}

/// The manifest's bytes, exactly as they are on disk.
///
/// Separate from [`read_manifest`] because a consent decision is recorded against the **document** the user agreed to,
/// and a parsed manifest cannot be hashed back to the bytes it came from — field order and whitespace are gone by then,
/// and two files that differ only in those would hash alike.
pub(crate) fn read_manifest_text(state: &AppState, plugin_id: &str) -> Option<String> {
    let path = state.paths.plugin_dir(plugin_id).join("manifest.json");
    std::fs::read_to_string(path).ok()
}

/// Parses a plugin identifier the interface sent.
///
/// # Errors
///
/// [`IpcError::InvalidArgument`].
pub(crate) fn parse(plugin_id: &str) -> Result<PluginId, IpcError> {
    PluginId::try_from(plugin_id.to_owned()).map_err(|error| {
        IpcError::unknown(synra_protocol::ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"pluginId": plugin_id, "detail": error.to_string()}))
    })
}
