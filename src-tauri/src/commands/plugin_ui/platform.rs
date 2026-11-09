use super::*;

/// The error code a refused platform action carries.
///
/// # Why the mapping is explicit rather than a `From`
///
/// Because `PlatformError`'s vocabulary is the platform's and `ErrorCode`'s is the wire's, and the two disagree on the
/// two cases a plugin acts on: a rate limit tells it to wait, a quota tells it to stop asking, and an oversized value
/// tells it to send less. A mechanical conversion would collapse them into "internal" and lose exactly the part a
/// plugin could recover from.
pub(crate) fn platform_error(error: synra_platform::PlatformError) -> IpcError {
    use synra_platform::PlatformError;

    if let Some(params) = error.command_details() {
        let code = match error {
            PlatformError::RunnerBusy { .. } => ErrorCode::RateLimited,
            PlatformError::RunnerTimedOut { .. } => ErrorCode::Timeout,
            PlatformError::RunnerOutputTooLong { .. } => ErrorCode::PayloadTooLarge,
            _ => ErrorCode::Internal,
        };
        return IpcError::unknown(code).with_params(params);
    }

    match error {
        PlatformError::RateLimited { remaining_ms, .. } => {
            IpcError::unknown(ErrorCode::RateLimited)
                .with_params(serde_json::json!({"retryAfterMs": remaining_ms}))
        }
        PlatformError::QuotaExceeded { used, limit, .. } => {
            IpcError::unknown(ErrorCode::RateLimited)
                .with_params(serde_json::json!({"quota": "platform", "used": used, "limit": limit}))
        }
        PlatformError::TooLong { field, len, max } => IpcError::unknown(ErrorCode::PayloadTooLarge)
            .with_params(serde_json::json!({"field": field, "len": len, "max": max})),
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
pub(crate) fn clipboard_read(
    state: &AppState,
    session: &ValidatedSession,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "platform:read-clipboard", "clipboard.read")?;
    let text = state.platform.read_clipboard().map_err(platform_error)?;
    Ok(text.map_or(serde_json::Value::Null, serde_json::Value::String))
}

/// The clipboard, written through the platform runner.
pub(crate) fn clipboard_write(
    state: &AppState,
    session: &ValidatedSession,
    args: &serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    require_grant(session, "platform:write-clipboard", "clipboard.write")?;
    let text = string_arg(args, "text")?;
    state
        .platform
        .write_clipboard(text)
        .map_err(platform_error)?;
    Ok(serde_json::Value::Null)
}

/// A URL in the user's browser, after the platform's own scheme checks.
pub(crate) fn platform_open_url(
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
pub(crate) fn platform_notify(
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
pub(crate) fn call_error(error: &synra_plugin_host::host_calls::CallError) -> IpcError {
    plugin_error(error.code, &error.message, error.details.as_deref())
}

/// Adapt the shared plugin failure while retaining the page's IPC error shape.
pub(crate) fn plugin_error(code: &str, message: &str, details: Option<&str>) -> IpcError {
    plugin_refusal(synra_plugin_host::engine::reported_failure(
        code, message, details,
    ))
}

/// A shared execution or host failure at the plugin page boundary.
pub(crate) fn plugin_refusal(error: synra_protocol::SynraError) -> IpcError {
    IpcError::unknown(error.code)
        .with_params(serde_json::json!({"reason": error.message}))
        .with_details(
            error
                .details
                .map(|details| *details)
                .unwrap_or(serde_json::Value::Null),
        )
}

/// One request through the host's fetcher.
///
/// # What is refused here and why
///
/// A per-request `timeoutMs` because the kernel's limit is the client's, not the caller's — accepting it and using the
/// default would be a promise nothing keeps. A byte body because the wire request carries a string; the façade refuses
/// both before the call, and this is the second half of each answer.
pub(crate) fn network_fetch(
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
    let method = init
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("GET")
        .to_owned();
    let headers = init
        .get("headers")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    if !headers.is_object() {
        return Err(IpcError::unknown(ErrorCode::InvalidArgument).with_params(
            serde_json::json!({"field": "headers", "reason": "an object is required"}),
        ));
    }
    let body =
        match init.get("body") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(text)) => Some(text.clone()),
            Some(_) => return Err(IpcError::unknown(ErrorCode::InvalidArgument).with_params(
                serde_json::json!({
                    "field": "body",
                    "reason": "a string body is required; bytes are not supported by this channel",
                }),
            )),
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
