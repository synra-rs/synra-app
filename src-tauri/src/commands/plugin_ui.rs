//! The plugin page's two calls: "I have drawn" and "do this for me".
//!
//! # Why one takes a label and the other a token
//!
//! `plugin_ui_ready` is the page's first word, and it presents the label the host gave its window: `validate` needs the
//! nonce, the claiming plugin and the version, and only the host's own table can supply the last two. The token is still
//! checked — it is the entry's own nonce.
//!
//! `plugin_ui_rpc` presents the **token**, because a label is not a secret: labels are deterministic (`plugin-<id>`), so a
//! page could guess another plugin's. The host finds the entry whose nonce matches the presented token, then validates the
//! pair; a token no hosted page holds is refused before any method is looked at.
//!
//! # What it refuses, and what it deliberately does not say
//!
//! A token nobody holds, and a token the registry rejects, are both refusals with the same code and the same parameters.
//! Saying which of the two applies would let a page probe for values that exist — the enumeration a token is supposed to
//! prevent.

use crate::state::AppState;
use synra_capability::Capability;
use synra_ipc::{IpcError, PluginUiOp};
use synra_platform::Platform as _;
use synra_plugin_ui::ValidatedSession;
use synra_protocol::ErrorCode;
use synra_protocol::ids::PluginId;

/// Records that a plugin page has mounted, after checking it holds a live session.
///
/// # Errors
///
/// [`ErrorCode::CapabilityDenied`] when the label is not one this shell hosted, or when the registry refuses the token —
/// see the module docs for why the two are not distinguished. [`ErrorCode::Internal`] is not refused here: validation does
/// not touch storage.
pub fn plugin_ui_ready(state: &AppState, label: &str) -> Result<(), IpcError> {
    let refused = || {
        IpcError::unknown(ErrorCode::CapabilityDenied)
            .with_params(serde_json::json!({"reason": "this page holds no live session"}))
    };

    // The three facts are **copied out** and the table's lock released before validation: `validate` takes the registry's
    // own lock, and holding two locks while a third party could take them in the other order is how a deadlock is built.
    // The window between the copy and the write is safe because the only thing that removes an entry is the window that
    // created it, and that same window is the one blocked on this command.
    let copy = {
        let hosted = state.hosted_ui.lock();
        hosted
            .get(label)
            .map(|entry| (entry.nonce, entry.plugin_id.clone(), entry.version.clone()))
    };
    let Some((nonce, plugin_id, version)) = copy else {
        return Err(refused());
    };

    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    state.ui_sessions.validate(nonce, &plugin_id, &version, now).map_err(|_| refused())?;

    if let Some(entry) = state.hosted_ui.lock().get_mut(label) {
        entry.ready = true;
    }
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
        hosted
            .values()
            .find(|entry| entry.nonce == nonce)
            .map(|entry| (entry.plugin_id.clone(), entry.version.clone()))
    };
    let Some((plugin_id, version)) = copy else {
        return Err(refused());
    };

    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    let session = state
        .ui_sessions
        .validate(nonce, &plugin_id, &version, now)
        .map_err(|_| refused())?;

    let Some(op) = PluginUiOp::parse(method) else {
        return Err(IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "method": method,
            "reason": "this host has no such operation",
        })));
    };

    match op {
        PluginUiOp::DeviceList => device_targets(state),
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
fn device_targets(state: &AppState) -> Result<serde_json::Value, IpcError> {
    let snapshot = super::devices::devices_snapshot(state)?;
    let targets: Vec<serde_json::Value> = snapshot
        .devices
        .iter()
        .map(|card| {
            serde_json::json!({
                "deviceId": card.device_id.to_string(),
                "displayName": card.display_name,
                "platform": card.platform,
                "online": card.online,
            })
        })
        .collect();
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
fn require_grant(session: &ValidatedSession, capability: &str, method: &str) -> Result<(), IpcError> {
    let wanted = Capability::parse(capability).map_err(internal)?;
    if session.grants().contains(&wanted) {
        return Ok(());
    }
    Err(IpcError::unknown(ErrorCode::CapabilityDenied).with_params(serde_json::json!({
        "method": method,
        "capability": capability,
    })))
}

/// One required string argument.
fn string_arg<'a>(args: &'a serde_json::Value, field: &str) -> Result<&'a str, IpcError> {
    args.get(field).and_then(serde_json::Value::as_str).ok_or_else(|| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"field": field, "reason": "a string is required"}))
    })
}

/// A database failure is this host's problem, not the plugin's.
fn internal(error: impl std::fmt::Display) -> IpcError {
    IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({"detail": error.to_string()}))
}

/// How many bytes this plugin's storage may hold.
///
/// # Why the set's quota and a default for the unloaded case
///
/// Because a UI-only plugin is never loaded into the engine — it has no Wasm — so the set has no quota for it, and the
/// default is what `HostCalls` would use for a plugin with no tighter one. A hardcoded number here would be a second
/// answer to a question the quota system already answers.
fn storage_limit(state: &AppState, plugin: &PluginId) -> u64 {
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
fn storage_get(
    state: &AppState,
    session: &ValidatedSession,
    plugin: &PluginId,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "storage:local:read", "storage.get")?;
    let key = string_arg(args, "key")?;
    let value = state.db().storage_get(plugin.as_str(), key).map_err(internal)?;
    // `null` rather than an absent field: the client's return is `string | null`, and "no value" is an answer.
    Ok(value.map_or(serde_json::Value::Null, serde_json::Value::String))
}

/// Writes one value, refusing a write that would exceed the plugin's quota.
fn storage_set(
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
        return Err(IpcError::unknown(ErrorCode::PayloadTooLarge).with_params(serde_json::json!({
            "quota": "storage",
            "usedBytes": used,
            "limitBytes": limit,
            "valueBytes": value.len(),
        })));
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
fn storage_after(used: u64, replaced: u64, new_len: usize) -> u64 {
    used.saturating_sub(replaced).saturating_add(new_len as u64)
}

/// Removes one value and answers whether there was one.
fn storage_delete(
    state: &AppState,
    session: &ValidatedSession,
    plugin: &PluginId,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "storage:local:write", "storage.delete")?;
    let key = string_arg(args, "key")?;
    let removed = state.db().storage_delete(plugin.as_str(), key).map_err(internal)?;
    Ok(serde_json::Value::Bool(removed))
}

/// Every key the page holds, optionally under a prefix.
fn storage_keys(
    state: &AppState,
    session: &ValidatedSession,
    plugin: &PluginId,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "storage:local:read", "storage.keys")?;
    let prefix = args.get("prefix").and_then(serde_json::Value::as_str);
    let keys = state.db().storage_keys(plugin.as_str(), prefix).map_err(internal)?;
    Ok(serde_json::Value::Array(
        keys.into_iter().map(serde_json::Value::String).collect(),
    ))
}

/// How much of the quota the page is using.
fn storage_quota(
    state: &AppState,
    session: &ValidatedSession,
    plugin: &PluginId,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "storage:local:read", "storage.quota")?;
    let used = state.db().storage_used_bytes(plugin.as_str()).map_err(internal)?;
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

/// The error code a refused platform action carries.
///
/// # Why the mapping is explicit rather than a `From`
///
/// Because `PlatformError`'s vocabulary is the platform's and `ErrorCode`'s is the wire's, and the two disagree on the
/// two cases a plugin acts on: a rate limit tells it to wait, a quota tells it to stop asking, and an oversized value
/// tells it to send less. A mechanical conversion would collapse them into "internal" and lose exactly the part a
/// plugin could recover from.
fn platform_error(error: synra_platform::PlatformError) -> IpcError {
    use synra_platform::PlatformError;

    match error {
        PlatformError::RateLimited { remaining_ms, .. } => {
            IpcError::unknown(ErrorCode::RateLimited)
                .with_params(serde_json::json!({"retryAfterMs": remaining_ms}))
        }
        PlatformError::QuotaExceeded { used, limit, .. } => IpcError::unknown(ErrorCode::RateLimited)
            .with_params(serde_json::json!({"quota": "platform", "used": used, "limit": limit})),
        PlatformError::TooLong { field, len, max } => {
            IpcError::unknown(ErrorCode::PayloadTooLarge)
                .with_params(serde_json::json!({"field": field, "len": len, "max": max}))
        }
        PlatformError::Unsupported(what) => IpcError::unknown(ErrorCode::Unsupported)
            .with_params(serde_json::json!({"reason": what})),
        // A refused URL or path is the caller's argument: the scheme, the credentials or the shape of it.
        PlatformError::BadUrl { reason, .. } => IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"field": "url", "reason": reason.to_string()})),
        PlatformError::BadPath { reason, .. } => IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"field": "path", "reason": reason.to_string()})),
        other => internal(other),
    }
}

/// The clipboard, read through the platform runner.
fn clipboard_read(state: &AppState, session: &ValidatedSession) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "platform:read-clipboard", "clipboard.read")?;
    let text = state.platform.read_clipboard().map_err(platform_error)?;
    Ok(text.map_or(serde_json::Value::Null, serde_json::Value::String))
}

/// The clipboard, written through the platform runner.
fn clipboard_write(
    state: &AppState,
    session: &ValidatedSession,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "platform:write-clipboard", "clipboard.write")?;
    let text = string_arg(args, "text")?;
    state.platform.write_clipboard(text).map_err(platform_error)?;
    Ok(serde_json::Value::Null)
}

/// A URL in the user's browser, after the platform's own scheme checks.
fn platform_open_url(
    state: &AppState,
    session: &ValidatedSession,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "platform:open-url", "platform.openUrl")?;
    let url = string_arg(args, "url")?;
    state.platform.open_url(url).map_err(platform_error)?;
    Ok(serde_json::Value::Null)
}

/// A visible notification, subject to the platform's hourly ceiling.
fn platform_notify(
    state: &AppState,
    session: &ValidatedSession,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "platform:notify", "platform.notify")?;
    let title = string_arg(args, "title")?;
    let body = string_arg(args, "body")?;
    state.platform.notify(title, body).map_err(platform_error)?;
    Ok(serde_json::Value::Null)
}

// ── Outbound network ────────────────────────────────────────────────────────────────────────────
//
// ★ Why this goes through `PluginSet`'s host rather than building a fetcher here.
//
// `Host::fetch` is the same path the Wasm half uses: it reads the plugin's `network:fetch:<host>` grants, applies one
// in-flight limit per plugin and reuses one client. A second fetcher in this module would have its own limit and its own
// reading of the grants, so a page and an instance together could exceed what neither may alone — and the two could
// disagree about which hosts the user allowed.

/// The wire code a host-call refusal carries, as an IPC code.
///
/// # Why this maps both spellings
///
/// `CallError::CAPABILITY_DENIED` is `capabilityDenied` (the host-call vocabulary), while a fetch guard's refusal uses
/// the protocol's `capability_denied` (snake case). Both reach this function, and an unmapped code would become
/// `Internal` — the one answer that tells a plugin nothing it can act on.
fn call_error(error: &synra_plugin_host::host_calls::CallError) -> IpcError {
    IpcError::unknown(ipc_code(error.code)).with_params(serde_json::json!({"reason": error.message}))
}

/// The protocol code for a host-call refusal's wire string.
///
/// # Why both spellings
///
/// `CallError::CAPABILITY_DENIED` is `capabilityDenied` (the host-call vocabulary), while a fetch guard's refusal uses
/// the protocol's `capability_denied` (snake case). Both reach this function, and an unmapped code would become
/// `Internal` — the one answer that tells a plugin nothing it can act on.
fn ipc_code(wire: &str) -> ErrorCode {
    match wire {
        "capabilityDenied" | "capability_denied" => return ErrorCode::CapabilityDenied,
        "badArgument" | "invalid_argument" => return ErrorCode::InvalidArgument,
        "hostNotAllowed" | "host_not_allowed" => return ErrorCode::HostNotAllowed,
        "ipNotAllowed" | "ip_not_allowed" => return ErrorCode::IpNotAllowed,
        "schemeNotAllowed" | "scheme_not_allowed" => return ErrorCode::SchemeNotAllowed,
        "redirectBlocked" | "redirect_blocked" => return ErrorCode::RedirectBlocked,
        "responseTooLarge" | "response_too_large" => return ErrorCode::ResponseTooLarge,
        "timeout" => return ErrorCode::Timeout,
        "rateLimited" | "rate_limited" => return ErrorCode::RateLimited,
        "unknownFunction" | "unsupported" => return ErrorCode::Unsupported,
        _ => {}
    }

    // ★ The rest are already in the protocol's own spelling: a plugin's `Failed` carries "one of the kernel's stable
    // error codes", and `ErrorCode` is the type that defines them. Parsing the string through it means a code added to
    // the protocol reaches this function without a second list to keep in step — and an unknown one is still `Internal`.
    serde_json::from_value::<ErrorCode>(serde_json::Value::String(wire.to_owned())).unwrap_or(ErrorCode::Internal)
}

/// One request through the host's fetcher.
///
/// # What is refused here and why
///
/// A per-request `timeoutMs` because the kernel's limit is the client's, not the caller's — accepting it and using the
/// default would be a promise nothing keeps. A byte body because the wire request carries a string; the façade refuses
/// both before the call, and this is the second half of each answer.
fn network_fetch(
    state: &AppState,
    plugin: &PluginId,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    let input = string_arg(args, "input")?;
    let init = args.get("init").cloned().unwrap_or(serde_json::Value::Null);
    if init.get("timeoutMs").is_some() {
        return Err(IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
            "reason": "a per-request timeout is not implemented; the host's own ceiling applies",
        })));
    }
    let method = init.get("method").and_then(serde_json::Value::as_str).unwrap_or("GET").to_owned();
    let headers = init.get("headers").cloned().unwrap_or_else(|| serde_json::json!({}));
    if !headers.is_object() {
        return Err(IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"field": "headers", "reason": "an object is required"})));
    }
    let body = match init.get("body") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(_) => {
            return Err(IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
                "field": "body",
                "reason": "a string body is required; bytes are not supported by this channel",
            })))
        }
    };

    let request = serde_json::json!({
        "url": input,
        "method": method,
        "headers": headers,
        "body": body,
    });

    // The set's lock is released before the request: a fetch held under it would make every other plugin command wait
    // for the network.
    let host = {
        let set = state.plugin_set()?;
        let guard = set.lock();
        std::sync::Arc::clone(guard.host())
    };
    let answer = host
        .fetch(plugin, &request.to_string())
        .map_err(|error| call_error(&error))?;
    serde_json::from_str(&answer).map_err(internal)
}

// ── The plugin's own actions ────────────────────────────────────────────────────────────────────
//
// ★ The two halves of "UI asks headless to do it". The design's reason is the direction of trust: the UI half is a page
// in a WebView, which the kernel cannot interrupt; the headless half is Wasm, which it can. So a high-risk capability is
// reached by asking the headless half, and this channel is the asking.
//
// The check that makes it safe is **the action belongs to this plugin**: the page holds a token for one plugin, and the
// name it passes is looked up in that plugin's declaration table. Without that lookup a page could name any action and
// the call would be one plugin driving another.

/// The host every plugin instance reaches, with the set's lock released.
fn host_of(state: &AppState) -> Result<std::sync::Arc<dyn synra_plugin_host::host_calls::Host>, IpcError> {
    let set = state.plugin_set()?;
    let guard = set.lock();
    Ok(std::sync::Arc::clone(guard.host()))
}

/// The actions this plugin declared, as `ActionDescriptor[]`.
fn self_actions(state: &AppState, plugin: &PluginId) -> Result<serde_json::Value, IpcError> {
    let host = host_of(state)?;
    let answer = host.action_list(plugin).map_err(|error| call_error(&error))?;
    serde_json::from_str(&answer).map_err(internal)
}

/// Calls one of this plugin's own actions on its headless half.
fn self_invoke(
    state: &AppState,
    plugin: &PluginId,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    let action = string_arg(args, "action")?;
    let arguments = args.get("args").cloned().unwrap_or(serde_json::Value::Null);

    // ★ The ownership check, before the call rather than inside it. `Work::InvokeAction` names an action and nothing
    // about which plugin declared it; a name that this plugin never declared would reach its `actions.invoke` export
    // anyway — and the design's rule is that the kernel validates it belongs to this plugin, so the check is here.
    let declared = state.db().plugin_actions(plugin.as_str()).map_err(internal)?;
    if !declared.iter().any(|row| row.name == action) {
        return Err(IpcError::unknown(ErrorCode::InvalidArgument).with_params(serde_json::json!({
            "field": "action",
            "reason": "this plugin has not declared that action",
        })));
    }

    let now_ms = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    let outcome = {
        let set = state.plugin_set()?;
        let mut guard = set.lock();
        let budget = guard
            .quota(plugin)
            .map_or_else(|| synra_plugin_host::CallBudget::from_quota(&synra_plugin_host::quota::Quota::default()), |quota| {
                synra_plugin_host::CallBudget::from_quota(&quota)
            });
        guard
            .call(
                plugin,
                &synra_plugin_host::Work::InvokeAction { name: action.to_owned(), args_json: arguments.to_string() },
                budget,
                now_ms,
            )
            .map_err(plugin_set_error)?
    };

    match outcome {
        // The contract says `unknown`, and the payload is a JSON string; a plugin that returned invalid JSON is handing
        // back text, and the honest reading of that is a string rather than an error the page cannot act on.
        synra_plugin_host::CallOutcome::Ok(payload) => Ok(
            serde_json::from_str(&payload).unwrap_or(serde_json::Value::String(payload)),
        ),
        synra_plugin_host::CallOutcome::Failed { code, message } => {
            Err(IpcError::unknown(ipc_code(&code)).with_params(serde_json::json!({"reason": message})))
        }
        synra_plugin_host::CallOutcome::Timeout { after } => Err(IpcError::unknown(ErrorCode::PluginTimeout)
            .with_params(serde_json::json!({"afterMs": after.as_millis()}))),
        other => Err(IpcError::unknown(ErrorCode::PluginTrapped)
            .with_params(serde_json::json!({"reason": format!("{other:?}")}))),
    }
}

/// The IPC answer for a set that would not make the call.
///
/// # Why these are not all `internal`
///
/// Because the fixes differ: a plugin that is not loaded in this shell is a property of this build, an instance limit is
/// "try again", and an engine refusal is a bug or a bad artifact. Collapsing them to `internal` would make the page's
/// `catch` unable to say which.
fn plugin_set_error(error: synra_core::dispatch::PluginSetError) -> IpcError {
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

/// The façade a plugin page runs: built by `pnpm run build:facade`, embedded here.
///
/// # Why the built file is committed
///
/// Because a Rust build must not need Node. The alternative — generating it in the Rust build — would make `cargo check`
/// depend on pnpm, and a contributor without the frontend toolchain could not compile the shell. The file is generated
/// and checked instead: `scripts/check-shapes.ts` rebuilds it and fails if it disagrees with this one, so it cannot drift
/// from `src/ipc/injection/bootstrap.ts` without a red gate.
const FACADE: &str = include_str!("../../inject/facade.js");

/// The facts the shell writes into a plugin page before any of its own scripts run.
///
/// # Why a JSON object rather than a series of assignments
///
/// Because the façade reads **all** of it, and a missing field would leave a namespace answering `undefined` — the exact
/// class of failure the injected-surface tests exist to prevent. Serialising one struct means the Rust side and
/// `InjectionSeed` in `bootstrap.ts` are one shape, and a change to either is visible as a diff to the other.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InjectionSeed<'a> {
    label: &'a str,
    token: String,
    plugin_id: &'a str,
    granted: &'a [String],
    denied_capabilities: &'a [String],
    locale: &'a str,
    dark_mode: bool,
    sdk_version: &'a str,
    platform: &'a str,
    host_kind: &'a str,
    app_version: &'a str,
}

/// The script a plugin page is started with: the facade inside a function, with the seed as its argument.
///
/// # Why the token is a **string**
///
/// Because it is a `u128`, and JavaScript numbers are exact only to 2^53. A nonce that silently loses its low digits is a
/// token that validates for nobody, and the failure would look like "the page never became ready" rather than like a
/// rounding error.
///
/// # Why the seed is an argument rather than a global
///
/// The design document's rule is that the token lives in a closure, not on an object. A `window.__SYNRA_PLUGIN__` would
/// be readable by any script the plugin's page includes — an analytics bundle, a dependency with a bad day — and a token
/// that can be read is a session that can be used from somewhere else. Here the seed is the wrapper function's
/// parameter: it exists only for the lifetime of the call, the facade copies what it needs, and nothing can look it up
/// afterwards. A page script can still *use* `window.synra`, which is the intended surface.
///
/// # Why the facade is inline rather than imported
///
/// Because an initialization script has no module loader: Tauri evaluates one script before the page's own, and an
/// `import()` would resolve after the plugin's first line has already run. The built file is an IIFE, so `var
/// SynraInjection` is a local of the wrapper and `SynraInjection.boot(seed)` runs in place — `window.synra` exists by
/// the time the page's code starts.
///
/// # Errors
///
/// [`ErrorCode::Internal`] if the seed cannot be serialised, which for these field types cannot happen — and is an error
/// rather than an `unwrap` because the workspace denies the panic.
fn initialization_script(facade: &str, seed: &InjectionSeed<'_>) -> Result<String, IpcError> {
    let json = serde_json::to_string(seed).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })?;
    Ok(format!(
        "(function(seed){{{facade}\nSynraInjection.boot(seed);}})({json});"
    ))
}

/// The platform name the plugin contract uses.
///
/// # Why an unknown platform is a refusal
///
/// Because the client's `Platform` is a closed union of five values, and a sixth name would be a value a plugin's type
/// says is impossible. A plugin that branches on the platform would take the `default` path on a machine this host does
/// not have a name for — a plausible wrong answer, which is worse than "this page cannot open here".
fn platform_name() -> Result<&'static str, IpcError> {
    Ok(match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        "linux" => "linux",
        "android" => "android",
        "ios" => "ios",
        other => {
            return Err(IpcError::unknown(ErrorCode::Unsupported).with_params(serde_json::json!({
                "platform": other,
                "reason": "the plugin contract has no name for this platform",
            })))
        }
    })
}

/// What the shell needs in order to open one plugin page.
///
/// The parts are separated from the Tauri call on purpose: a URL, a label and a script are values a test can assert, and
/// `WebviewWindowBuilder` is a call no test here can make. Everything that could be wrong in an interesting way is in this
/// struct; what is left in `shell.rs` is five lines of plumbing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    /// The WebView's label, and the key `plugin_ui_ready` is called with.
    pub label: String,
    /// The page's address, under the plugin origin: `synra-plugin://<plugin-id>/<page>`.
    pub url: String,
    /// The initialization script carrying the token and the label.
    pub script: String,
}

/// Prepares the page for one plugin, and records who it belongs to.
///
/// # Errors
///
/// [`ErrorCode::PluginNotInstalled`] when there is no manifest to read, [`ErrorCode::Unsupported`] when the plugin has no
/// UI half. Both are refusals a caller can act on: the first says "install it", the second says "this one has no window".
pub fn open_request(state: &AppState, plugin_id: &str) -> Result<OpenRequest, IpcError> {
    use synra_protocol::ErrorCode;

    let id = super::plugins::parse(plugin_id)?;
    let manifest = super::plugins::read_manifest(state, &id.to_string()).ok_or_else(|| {
        IpcError::unknown(ErrorCode::PluginNotInstalled)
            .with_params(serde_json::json!({"pluginId": plugin_id}))
    })?;
    let Some(ui) = manifest.runtime.ui.as_ref() else {
        return Err(IpcError::unknown(ErrorCode::Unsupported)
            .with_params(serde_json::json!({"pluginId": plugin_id, "reason": "it has no UI"})));
    };

    // ★ The page key is the manifest's default page when it names one, and the entry's file stem otherwise: the address
    // shape is `synra-plugin://<plugin-id>/<page>` and `entry` is a path inside the bundle (`dist/index.html`), which is
    // not a page key.
    let page = ui.default_page.clone().unwrap_or_else(|| {
        std::path::Path::new(&ui.entry)
            .file_stem()
            .map_or_else(|| "index".to_owned(), |stem| stem.to_string_lossy().into_owned())
    });
    let label = format!("plugin-{}", id.as_str().replace('/', "__"));
    let url = format!("synra-plugin://{}/{page}", id.as_str());

    let now = u64::try_from(synra_store::sqlite::now_ms()).unwrap_or_default();
    // ★ The grants the page will hold, read from the decisions the user actually made. An empty set here made every
    // page powerless no matter what it had been granted — `env.can` would answer false for everything, and a gate built
    // on the session would refuse calls the user had allowed.
    let decisions = state.db().plugin_grants(id.as_str()).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"detail": error.to_string()}))
    })?;
    let granted: Vec<String> = decisions
        .iter()
        .filter(|row| row.granted)
        .map(|row| row.capability.clone())
        .collect();
    let denied: Vec<String> = decisions
        .iter()
        .filter(|row| !row.granted)
        .map(|row| row.capability.clone())
        .collect();
    let grants = synra_capability::CapabilitySet::parse_all(granted.iter().map(String::as_str)).map_err(|error| {
        // A decision the user made names a capability this build cannot parse, so the page's permissions are unknown.
        // Opening it with a guessed set would be worse than refusing: the plugin would run with permissions nobody chose.
        IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({
            "pluginId": plugin_id,
            "reason": format!("a recorded grant is not a capability this build knows: {error}"),
        }))
    })?;
    // ★ A random nonce. `issue_placeholder` is a counter — its own documentation says it is not for production — and a
    // page that can guess another page's token can call as that plugin. The window is the one place a real nonce can be
    // made, so it is made here.
    let nonce = state.ui_sessions.issue_with(
        &id,
        manifest.version.clone(),
        grants,
        uuid::Uuid::new_v4().as_u128(),
        now,
    );

    // ★ The environment the page's façade answers from. These are the host's facts, decided once, before the page
    // loads: a round trip to learn something the shell already holds is a round trip that can fail, and a render that has
    // to await `env.platform()` is a render that cannot branch synchronously.
    let config = super::config::config_get(state)?;
    let locale = config
        .values
        .get("ui.locale")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("en")
        .to_owned();
    let dark_mode = config.values.get("ui.theme").and_then(serde_json::Value::as_str) == Some("dark");
    // The host the plugin was built against, which is what `env.sdkVersion()` answers; `min` is the compatibility floor,
    // and reporting it would tell the plugin it was built against an older host than it was.
    let sdk_version = manifest
        .sdk
        .preferred
        .clone()
        .unwrap_or_else(|| manifest.sdk.min.clone());
    let platform = platform_name()?;
    let host_kind = if cfg!(any(target_os = "android", target_os = "ios")) {
        "mobile"
    }
    else {
        "desktop"
    };

    let seed = InjectionSeed {
        label: &label,
        token: nonce.to_string(),
        plugin_id: id.as_str(),
        granted: &granted,
        denied_capabilities: &denied,
        locale: &locale,
        dark_mode,
        sdk_version: &sdk_version,
        platform,
        host_kind,
        app_version: env!("CARGO_PKG_VERSION"),
    };
    let script = initialization_script(FACADE, &seed)?;

    state.hosted_ui.lock().insert(
        label.clone(),
        crate::state::HostedUi { plugin_id: id, version: manifest.version, nonce, ready: false },
    );

    Ok(OpenRequest { label, url, script })
}

/// One file out of a plugin's extracted UI bundle.
///
/// # What this is, and what it is not
///
/// It is the **policy** half of the `synra-plugin://` handler: which paths are legal, which content type a file gets, and
/// that the joined path is still inside the bundle. It is not the Tauri handler, because a handler needs a running
/// application and this needs a directory — so the interesting failure (a page asking for `../../identity.toml`) is a unit
/// test rather than a manual experiment.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] for a path the artifact rules refuse or one that escapes the root, and
/// [`ErrorCode::Internal`] when the file cannot be read. The two are different answers: the first is somebody's bug or an
/// attack, the second is a broken install.
pub fn ui_file(root: &std::path::Path, path: &str) -> Result<(Vec<u8>, &'static str), IpcError> {
    use synra_protocol::ErrorCode;

    synra_plugin_ui::artifact::check_path(path).map_err(|error| {
        IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"path": path, "reason": error.to_string()}))
    })?;

    let file = root.join(path);
    // ★ Belt **and** braces: `check_path` already refuses `..` and absolute paths, and this refuses a join that escaped
    // anyway. A path check that is the only guard is one refactor away from not being a guard.
    if !synra_store::is_within(root, &file) {
        return Err(IpcError::unknown(ErrorCode::InvalidArgument)
            .with_params(serde_json::json!({"path": path, "reason": "it leaves the bundle"})));
    }

    let bytes = std::fs::read(&file).map_err(|error| {
        IpcError::unknown(ErrorCode::Internal)
            .with_params(serde_json::json!({"path": path, "reason": error.to_string()}))
    })?;
    // A file nobody said the type of is still a file: `application/octet-stream` makes the WebView download it rather than
    // guess, which is the honest default.
    let content_type = synra_plugin_ui::artifact::content_type_for(path).unwrap_or("application/octet-stream");
    Ok((bytes, content_type))
}

/// Splits a `synra-plugin://` address into the plugin it names and the path inside its bundle.
///
/// # Why this is not a `Url` parse
///
/// A plugin id contains a slash (`@acme/hello`), and the address is `synra-plugin://<plugin-id>/<path>` — so a URL parser
/// would read `@acme` as the host and `/hello/index.html` as the path, splitting the id in the wrong place. The split is
/// therefore made against the ids the host actually opened: the **longest** one that prefixes the address wins, and what
/// follows is the path.
///
/// Longest rather than first-match because two ids can share a prefix (`@acme/hello` and `@acme/hello-world`): first-match
/// would serve one plugin's files under the other's name.
#[must_use]
pub fn split_ui_url<'a>(url: &str, known: &'a [synra_protocol::ids::PluginId]) -> Option<(&'a synra_protocol::ids::PluginId, String)> {
    const SCHEME: &str = "synra-plugin://";
    let rest = url.strip_prefix(SCHEME)?;
    // A query or fragment is not part of a file path, and a cache-busting `?v=1` is common enough to allow.
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);

    let mut best: Option<(&'a synra_protocol::ids::PluginId, String)> = None;
    for id in known {
        let Some(after) = rest.strip_prefix(id.as_str()) else {
            continue;
        };
        let Some(path) = after.strip_prefix('/') else {
            continue;
        };
        let better = best.as_ref().is_none_or(|(winner, _)| id.as_str().len() > winner.as_str().len());
        if better {
            best = Some((id, path.to_owned()));
        }
    }
    best
}

/// The `synra-plugin://` handler: one bundle file, or a refusal.
///
/// # Why it is here and not in `shell.rs`
///
/// Because it is three decisions and one call: which ids a request may name, which file it may have, and what to answer
/// when the answer is no. Those are the same decisions [`split_ui_url`] and [`ui_file`] make, and they have tests; the
/// protocol registration in `shell.rs` is wiring.
#[cfg(feature = "tauri-shell")]
pub fn serve(
    app: &tauri::AppHandle,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<std::borrow::Cow<'static, [u8]>> {
    use std::borrow::Cow;
    use tauri::Manager as _;

    fn answer(status: u16, body: &'static str) -> tauri::http::Response<Cow<'static, [u8]>> {
        tauri::http::Response::builder()
            .status(status)
            .header(tauri::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Cow::Borrowed(body.as_bytes()))
            .unwrap_or_else(|_| tauri::http::Response::new(Cow::Borrowed(b"the response could not be built")))
    }

    let state = app.state::<AppState>();
    let uri = request.uri().to_string();

    // ★ The ids come from the host's own table, not from the request: a page may only name a plugin this shell opened a
    // window for, which is what makes the longest-prefix split safe.
    let known: Vec<synra_protocol::ids::PluginId> =
        state.hosted_ui.lock().values().map(|entry| entry.plugin_id.clone()).collect();

    let Some((plugin_id, path)) = split_ui_url(&uri, &known) else {
        return answer(404, "no page by that name");
    };
    let root = synra_plugin_ui::artifact::bundle_dir(&state.paths.plugin_dir(plugin_id.as_str()));

    match ui_file(&root, &path) {
        Ok((bytes, content_type)) => tauri::http::Response::builder()
            .status(200)
            .header(tauri::http::header::CONTENT_TYPE, content_type)
            .body(Cow::Owned(bytes))
            .unwrap_or_else(|_| answer(500, "the response could not be built")),
        Err(refusal) => {
            // 403 for a path the rules refuse, 404 for a file that is not there: the first is somebody asking for something
            // they may never have, the second is a bundle that is missing a file it should carry.
            let status = if refusal.code == synra_protocol::ErrorCode::InvalidArgument { 403 } else { 404 };
            answer(status, "no page by that name")
        }
    }
}

#[cfg(test)]
mod tests;
