use super::*;

/// How many bytes this plugin's storage may hold.
///
/// # Why the set's quota and a default for the unloaded case
///
/// Because a UI-only plugin is never loaded into the engine — it has no Wasm — so the set has no quota for it, and the
/// default is what `HostCalls` would use for a plugin with no tighter one. A hardcoded number here would be a second
/// answer to a question the quota system already answers.
pub(crate) fn storage_limit(state: &AppState, plugin: &PluginId) -> u64 {
    state
        .plugin_set()
        .map(|set| set.lock().quota(plugin))
        .ok()
        .flatten()
        .map_or_else(
            || synra_plugin_host::quota::Quota::default().max_storage_bytes,
            |quota| quota.max_storage_bytes,
        )
}

/// Reads one value out of the page's own storage.
pub(crate) fn storage_get(
    state: &AppState,
    session: &ValidatedSession,
    plugin: &PluginId,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "storage:local:read", "storage.get")?;
    let key = string_arg(args, "key")?;
    let value = state
        .db()
        .storage_get(plugin.as_str(), key)
        .map_err(internal)?;
    // `null` rather than an absent field: the client's return is `string | null`, and "no value" is an answer.
    Ok(value.map_or(serde_json::Value::Null, serde_json::Value::String))
}

/// Writes one value, refusing a write that would exceed the plugin's quota.
pub(crate) fn storage_set(
    state: &AppState,
    session: &ValidatedSession,
    plugin: &PluginId,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "storage:local:write", "storage.set")?;
    let key = string_arg(args, "key")?;
    let value = string_arg(args, "value")?;

    // The guard is released before the quota lookup: `plugins_install` takes the plugin set and *then* the database, and
    // a path that took them in the other order would be a deadlock waiting for two commands to overlap.
    let (used, replaced) = {
        let db = state.db();
        let used = db.storage_used_bytes(plugin.as_str()).map_err(internal)?;
        let replaced = db
            .storage_get(plugin.as_str(), key)
            .map_err(internal)?
            .map_or(0, |previous| previous.len() as u64);
        drop(db);
        (used, replaced)
    };
    let limit = storage_limit(state, plugin);
    // A write replaces, so the growth is the difference — the same arithmetic `HostCalls` does for the Wasm half.
    let after = storage_after(used, replaced, value.len());
    if after > limit {
        return Err(
            IpcError::unknown(ErrorCode::PayloadTooLarge).with_params(serde_json::json!({
                "quota": "storage",
                "usedBytes": used,
                "limitBytes": limit,
                "valueBytes": value.len(),
            })),
        );
    }

    state
        .db()
        .storage_set(plugin.as_str(), key, value, synra_store::sqlite::now_ms())
        .map_err(internal)?;
    Ok(serde_json::Value::Null)
}

/// How large the plugin's storage would be after this write.
///
/// # Why a function rather than an expression
///
/// Because the arithmetic is where a quota check goes wrong in the quiet direction: `used + new` would count a value
/// that is being **replaced** twice, and a plugin rewriting one key with a slightly larger value would be refused long
/// before its storage was actually near the limit. A named function is testable with small numbers, which the real
/// 64 MiB ceiling is not.
pub(crate) fn storage_after(used: u64, replaced: u64, new_len: usize) -> u64 {
    used.saturating_sub(replaced).saturating_add(new_len as u64)
}

/// Removes one value and answers whether there was one.
pub(crate) fn storage_delete(
    state: &AppState,
    session: &ValidatedSession,
    plugin: &PluginId,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "storage:local:write", "storage.delete")?;
    let key = string_arg(args, "key")?;
    let removed = state
        .db()
        .storage_delete(plugin.as_str(), key)
        .map_err(internal)?;
    Ok(serde_json::Value::Bool(removed))
}

/// Every key the page holds, optionally under a prefix.
pub(crate) fn storage_keys(
    state: &AppState,
    session: &ValidatedSession,
    plugin: &PluginId,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "storage:local:read", "storage.keys")?;
    let prefix = args.get("prefix").and_then(serde_json::Value::as_str);
    let keys = state
        .db()
        .storage_keys(plugin.as_str(), prefix)
        .map_err(internal)?;
    Ok(serde_json::Value::Array(
        keys.into_iter().map(serde_json::Value::String).collect(),
    ))
}

/// How much of the quota the page is using.
pub(crate) fn storage_quota(
    state: &AppState,
    session: &ValidatedSession,
    plugin: &PluginId,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "storage:local:read", "storage.quota")?;
    let used = state
        .db()
        .storage_used_bytes(plugin.as_str())
        .map_err(internal)?;
    Ok(serde_json::json!({
        "usedBytes": used,
        "maxBytes": storage_limit(state, plugin),
    }))
}

// ── The desktop: clipboard, browser, notifications ─────────────────────────────────────────────
//
// ★ Why these go through `AppState.platform` rather than constructing a platform per call.
//
// `CommandPlatform` carries per-instance state — the notification limiter is the one that matters — so a fresh instance
// per call would hand a plugin a second hourly allowance for the same user. The host and this channel share one.
