use super::*;

// The check that makes it safe is **the action belongs to this plugin**: the page holds a token for one plugin, and the
// name it passes is looked up in that plugin's declaration table. Without that lookup a page could name any action and
// the call would be one plugin driving another.

/// The host every plugin instance reaches, with the set's lock released.
pub(crate) fn host_of(
    state: &AppState,
) -> Result<std::sync::Arc<dyn synra_plugin_host::host_calls::Host>, IpcError> {
    let set = state.plugin_set()?;
    let guard = set.lock();
    Ok(std::sync::Arc::clone(guard.host()))
}

/// The actions this plugin declared, as `ActionDescriptor[]`.
pub(crate) fn self_actions(
    state: &AppState,
    plugin: &PluginId,
) -> Result<serde_json::Value, IpcError> {
    let host = host_of(state)?;
    let answer = host
        .action_list(plugin)
        .map_err(|error| call_error(&error))?;
    serde_json::from_str(&answer).map_err(internal)
}

/// Calls one of this plugin's own actions on its headless half.
pub(crate) fn self_invoke(
    state: &AppState,
    plugin: &PluginId,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    let action = string_arg(args, "action")?;
    let arguments = args.get("args").cloned().unwrap_or(serde_json::Value::Null);

    // ★ The ownership check, before the call rather than inside it. `Work::InvokeAction` names an action and nothing
    // about which plugin declared it; a name that this plugin never declared would reach its `actions.invoke` export
    // anyway — and the design's rule is that the kernel validates it belongs to this plugin, so the check is here.
    let declared = state
        .db()
        .plugin_actions(plugin.as_str())
        .map_err(internal)?;
    if !declared.iter().any(|row| row.name == action) {
        return Err(
            IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "field": "action",
                "reason": "this plugin has not declared that action",
            })),
        );
    }

    let now_ms = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    let outcome = {
        let set = state.plugin_set()?;
        let mut guard = set.lock();
        let budget = guard.quota(plugin).map_or_else(
            || {
                synra_plugin_host::CallBudget::from_quota(
                    &synra_plugin_host::quota::Quota::default(),
                )
            },
            |quota| synra_plugin_host::CallBudget::from_quota(&quota),
        );
        guard
            .call(
                plugin,
                &synra_plugin_host::Work::InvokeAction {
                    name: action.to_owned(),
                    args_json: arguments.to_string(),
                },
                budget,
                now_ms,
            )
            .map_err(plugin_set_error)?
    };

    let payload = outcome.into_result().map_err(plugin_refusal)?;
    Ok(serde_json::from_str(&payload).unwrap_or(serde_json::Value::String(payload)))
}

/// The IPC answer for a set that would not make the call.
///
/// # Why these are not all `internal`
///
/// Because the fixes differ: a plugin that is not loaded in this shell is a property of this build, an instance limit is
/// "try again", and an engine refusal is a bug or a bad artifact. Collapsing them to `internal` would make the page's
/// `catch` unable to say which.
pub(crate) fn plugin_set_error(error: synra_core::dispatch::PluginSetError) -> IpcError {
    use synra_core::dispatch::PluginSetError;

    match &error {
        PluginSetError::NotLoaded { id } => IpcError::unknown(ErrorCode::Unsupported).with_params(
            serde_json::json!({"pluginId": id.to_string(), "reason": "the plugin is not loaded in this shell"}),
        ),
        PluginSetError::AtInstanceLimit { id } => IpcError::unknown(ErrorCode::RateLimited).with_params(
            serde_json::json!({"pluginId": id.to_string(), "reason": "the plugin is at its instance limit"}),
        ),
        _ => internal(error),
    }
}
