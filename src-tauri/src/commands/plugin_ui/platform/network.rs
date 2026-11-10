use super::*;

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
