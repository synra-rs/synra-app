use super::platform::{
    clipboard_read, clipboard_write, network_fetch, platform_notify, platform_open_url,
    self_actions, self_invoke,
};
use super::*;

/// Records that a plugin page has mounted, after checking it holds a live session.
///
/// # Errors
///
/// [`ErrorCode::CapabilityDenied`] when the label is not one this shell hosted, or when the registry refuses the token —
/// see the module docs for why the two are not distinguished. [`ErrorCode::Internal`] reports an unreadable authority store.
pub fn plugin_ui_ready(state: &AppState, label: &str) -> Result<(), IpcError> {
    let copy = state
        .hosted_ui
        .lock()
        .get(label)
        .cloned()
        .ok_or_else(authority::refused)?;
    let session = authority::validate(state, &copy)?;
    let mut hosted = state.hosted_ui.lock();
    let entry = hosted
        .get_mut(label)
        .filter(|entry| entry.nonce == copy.nonce)
        .ok_or_else(authority::refused)?;
    entry.ready = true;
    session.token().mark_ready();
    drop(hosted);
    Ok(())
}

/// Answers one call from a plugin page, after checking the token it presents.
///
/// # Why the token alone is enough to find the page
///
/// Because the host issued it: [`open_request`] records the nonce beside the plugin it opened the window for, and this
/// looks the entry up by that nonce. The page never gets to say which plugin it is — it presents a value only the host
/// could have given it, and the host remembers the rest.
///
/// # What is implemented, and what is only addressable
///
/// `device.list` and `storage.*` have bodies. The rest of the surface answers [`ErrorCode::Unsupported`] — an honest
/// "this build does not do that yet", rather than an empty list or a promise nothing keeps.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] when the token is not a decimal number or an operation's arguments are malformed,
/// [`ErrorCode::CapabilityDenied`] when no hosted page holds the token, the session is stale, or a storage operation's
/// capability was not granted, [`ErrorCode::Unsupported`] for a method that is not on the plugin surface or is not built
/// here, and [`ErrorCode::PayloadTooLarge`] when a write would exceed the plugin's storage quota.
pub fn plugin_ui_rpc(
    state: &AppState,
    token: &str,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    dispatch(state, None, token, method, args)
}

/// [`plugin_ui_rpc`], with the managed state so a chunked transfer can finish on a worker.
///
/// # Why the two entries exist
///
/// Because only the Tauri command owns an `Arc<AppState>`, and a finished upload's send has to outlive the call. Everything
/// else is shared; the difference is one optional reference that only `transfer.finish` uses.
pub fn plugin_ui_rpc_arc(
    state: &std::sync::Arc<AppState>,
    token: &str,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    dispatch(state, Some(state), token, method, args)
}

pub(crate) fn dispatch(
    state: &AppState,
    arc: Option<&std::sync::Arc<AppState>>,
    token: &str,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    let refused = || {
        IpcError::unknown(ErrorCode::CapabilityDenied)
            .with_params(serde_json::json!({"reason": "this page holds no live session"}))
    };

    let nonce = token.parse::<u128>().map_err(|_| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"reason": "the token is not a number"}))
    })?;

    // The same copy-then-release shape as `plugin_ui_ready`: `validate` takes the registry's lock, and holding the two
    // in one expression is how a deadlock is built.
    let copy = {
        let hosted = state.hosted_ui.lock();
        hosted.values().find(|entry| entry.nonce == nonce).cloned()
    };
    let entry = copy.ok_or_else(refused)?;
    let session = authority::validate(state, &entry)?;
    let plugin_id = entry.plugin_id;

    let Some(op) = PluginUiOp::parse(method) else {
        return Err(
            IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
                "method": method,
                "reason": "this host has no such operation",
            })),
        );
    };

    match op {
        PluginUiOp::DeviceList => {
            require_grant(&session, "device:list", method)?;
            device_targets(state)
        }
        PluginUiOp::StorageGet => storage_get(state, &session, &plugin_id, args),
        PluginUiOp::StorageSet => storage_set(state, &session, &plugin_id, args),
        PluginUiOp::StorageDelete => storage_delete(state, &session, &plugin_id, args),
        PluginUiOp::StorageKeys => storage_keys(state, &session, &plugin_id, args),
        PluginUiOp::StorageQuota => storage_quota(state, &session, &plugin_id),
        PluginUiOp::ClipboardRead => clipboard_read(state, &session),
        PluginUiOp::ClipboardWrite => clipboard_write(state, &session, args),
        PluginUiOp::PlatformOpenUrl => platform_open_url(state, &session, args),
        PluginUiOp::PlatformNotify => platform_notify(state, &session, args),
        PluginUiOp::NetworkFetch => network_fetch(state, &plugin_id, args),
        PluginUiOp::SelfActions => self_actions(state, &plugin_id),
        PluginUiOp::SelfInvoke => self_invoke(state, &plugin_id, args),
        PluginUiOp::TransferSend => transfer_send(state, &session, args),
        PluginUiOp::TransferBegin => transfer_begin(state, &session, args),
        PluginUiOp::TransferChunk => transfer_chunk(state, &session, args),
        PluginUiOp::TransferFinish => transfer_finish(state, arc, &session, args),
        PluginUiOp::TransferAbort => transfer_abort(state, &session, args),
        PluginUiOp::TransferStatus => transfer_status(state, args),
        _ => Err(IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "method": method,
            "reason": "the operation is on the plugin surface and this build does not implement it",
        }))),
    }
}

/// The device list as a plugin page sees it: the four fields the client contract declares.
///
/// # Why the reduction happens here
///
/// Because the plugin's `DeviceTarget` is a **different type** from the host's `DeviceCard` on purpose: a card carries a
/// transfer summary, an address seen on the network, a blocked flag — things a plugin has no business knowing. Mapping in
/// the interface would put that judgement in JavaScript; mapping here means the page cannot receive what it may not see.
pub(crate) fn device_targets(state: &AppState) -> Result<serde_json::Value, IpcError> {
    let snapshot = crate::commands::devices::devices_snapshot(state)?;
    let targets: Vec<serde_json::Value> = snapshot
        .devices
        .iter()
        .filter(|card| card.paired && !card.blocked)
        .take(MAX_DEVICE_TARGETS + 1)
        .map(|card| {
            serde_json::json!({
                "deviceId": card.device_id.to_string(),
                "displayName": card.display_name,
                "platform": card.platform,
                "online": card.online,
            })
        })
        .collect();
    if targets.len() > MAX_DEVICE_TARGETS {
        return Err(IpcError::unknown(ErrorCode::PayloadTooLarge).with_params(
            serde_json::json!({"method": "device.list", "maxDevices": MAX_DEVICE_TARGETS}),
        ));
    }
    Ok(serde_json::Value::Array(targets))
}

// ── Storage ─────────────────────────────────────────────────────────────────────────────────────
//
// ★ Why this is a per-plugin table rather than a directory of files.
//
// Because it is the same table the headless half uses: a plugin that remembers a draft from its UI and reads it from its
// Wasm half is one plugin, and two stores would let the two halves disagree about what it remembers. The store's key is
// `(plugin_id, key)`, so one page cannot read another plugin's values even with a shared database.

/// Refuses a storage call the page's session does not hold the capability for.
///
/// # Why the session's set rather than the `granted` array the façade has
///
/// Because they are two representations of one decision, and the session's is the one the kernel validated. The façade's
/// array is for `env.can` (a synchronous answer with no round trip); a call that changes state goes through the gate.
pub(crate) fn require_grant(
    session: &ValidatedSession,
    capability: &str,
    method: &str,
) -> Result<(), IpcError> {
    let wanted = Capability::parse(capability).map_err(internal)?;
    if session.grants().contains(&wanted) {
        return Ok(());
    }
    Err(
        IpcError::unknown(ErrorCode::CapabilityDenied).with_params(serde_json::json!({
            "method": method,
            "capability": capability,
        })),
    )
}

/// One required string argument.
pub(crate) fn string_arg<'a>(
    args: &'a serde_json::Value,
    field: &str,
) -> Result<&'a str, IpcError> {
    args.get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            IpcError::unknown(ErrorCode::InvalidArgument)
                .with_params(serde_json::json!({"field": field, "reason": "a string is required"}))
        })
}

/// A database failure is this host's problem, not the plugin's.
pub(crate) fn internal(error: impl std::fmt::Display) -> IpcError {
    IpcError::unknown(ErrorCode::Internal)
        .with_params(serde_json::json!({"detail": error.to_string()}))
}
