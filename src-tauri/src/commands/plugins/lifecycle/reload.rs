use super::*;

/// Tries one plugin's load again without touching its flags.
///
/// # Why this exists
///
/// A faulted plugin is installed, enabled and trusted; what failed was starting it. The only way to ask again was to
/// switch it off and on, which is a flag change the user did not want and which the page had to explain as a workaround.
/// This is the same `apply_lifecycle` an enable runs, on a row that is already where it should be.
///
/// # Errors
///
/// [`IpcError::PluginNotInstalled`] when no row exists, and `Internal` when the list cannot be read. A load that fails is
/// **not** an error: the answer is the card, and its `stateReason` is the sentence the loader produced.
pub fn plugins_reload(state: &AppState, plugin_id: &str) -> Result<PluginSnapshot, IpcError> {
    let id = parse(plugin_id)?;
    let row = state
        .db()
        .plugins()
        .map_err(internal)?
        .into_iter()
        .find(|row| row.plugin_id == id.as_str())
        .ok_or_else(|| {
            IpcError::unknown(synra_protocol::ErrorCode::PluginNotInstalled)
                .with_params(serde_json::json!({"pluginId": plugin_id}))
        })?;

    if !state
        .db()
        .invalidate_plugin_authority(id.as_str())
        .map_err(internal)?
    {
        return Err(IpcError::unknown(
            synra_protocol::ErrorCode::PluginNotInstalled,
        ));
    }
    let _ = apply_lifecycle(state, &id);
    announce(state, &id);
    let loaded: Option<Vec<PluginId>> = state.plugin_set().ok().map(|set| set.lock().ids());
    snapshot(state, &row, &id, loaded.as_deref())
}
