import type { ErrorCode } from '@synra/protocol'

/** Retry policy mirrored from ErrorCode::retryable and checked against Rust serialization. */
export const ERROR_RETRYABLE = {
  capability_denied: false,
  plugin_not_installed: false,
  device_offline: true,
  not_paired: false,
  transport_not_open: true,
  transfer_failed: true,
  integrity_mismatch: false,
  payload_too_large: false,
  timeout: true,
  rate_limited: true,
  unsupported: false,
  invalid_argument: false,
  scheme_not_allowed: false,
  host_not_allowed: false,
  ip_not_allowed: false,
  redirect_blocked: false,
  response_too_large: false,
  protocol_violation: false,
  protocol_version_unsupported: false,
  plugin_trapped: false,
  plugin_timeout: false,
  internal: true
} satisfies Readonly<Record<ErrorCode, boolean>>

export function isErrorCode(value: unknown): value is ErrorCode {
  return typeof value === 'string' && Object.hasOwn(ERROR_RETRYABLE, value)
}
