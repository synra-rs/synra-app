use super::*;

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
