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
