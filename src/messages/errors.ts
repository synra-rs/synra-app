/**
 * What the interface says when the kernel refuses something (R33).
 *
 * Not an apology and not a shrug: what happened, in the interface's own words. The `code` is kept for the diagnostics
 * page — the main interface does not print raw error strings (R34).
 *
 * ★ **Two tables, and the order between them matters.** The host's `IpcError.message` is a **key**, not a sentence —
 * the interface is the side that renders it (`src/ipc/client.ts` explains why). A key is finer-grained than a code
 * (one code can have several messages), so `kernelErrors` is consulted first and `errors` is the fallback for codes
 * whose key we have not met yet. An unknown key is not blank: it falls back to the code's sentence, and then to
 * `unknown` — a chain that always says *something* true.
 */

/** The kernel's error codes, as far as the interface says anything about them. */
export type ErrorCode
  = | 'capability_denied'
    | 'device_offline'
    | 'invalid_argument'
    | 'not_found'
    | 'plugin_faulted'
    | 'unauthorized'
    | string

export const kernelErrors = {
  'error.identityHistoryMissing': 'The saved registration and its recovery record are missing. Restore a trusted database backup before repairing this identity.',
  'error.badArgument': 'Something in the request was not valid.',
  'error.bridgeFailed': 'The interface could not reach the kernel.',
  'error.capabilityDenied': 'This needs a permission that has not been granted.',
  'error.commandNotFound': 'That is not something this version can do.',
  'error.storeUnavailable': 'The place that keeps this is not available.',
  'error.timeout': 'The kernel did not answer in time.',
  // ★ The key `IpcError::unknown` builds for `Unsupported`, which had no sentence until a refusal needed one.
  //
  // The shell answers `unsupported` for the halves this build does not have — a scan with no discovery service, a plugin
  // with no engine — and every such refusal carries a `reason` in its `params`. The interface does not render `params`
  // (R34), so until this line existed the user read the fallback for an unknown *code*, "That did not work.", which says
  // nothing about whether to wait or give up.
  'error.unsupported': 'This version cannot do that yet.'
} as const

export const errors = {
  capability_denied: 'This needs a permission that has not been granted.',
  device_offline: 'That device is not answering.',
  invalid_argument: 'Something in the request was not valid.',
  not_found: 'That is not here any more.',
  plugin_faulted: 'That plugin stopped working.',
  unauthorized: 'This is not allowed.',
  // ★ The rest of `synra_protocol::ErrorCode`, for the codes a person can actually act on. Every one of these used to
  // fall through to `unknown` — "That did not work." — which says nothing about whether to install something, pair
  // first, pick a smaller file, or simply try again. `04 §6` asks for a cause plus a recovery step; a shared fallback
  // gives neither. Codes with no honest sentence yet (the plugin-sandbox ones: trapped, timeout, protocol violations)
  // deliberately stay out and keep the fallback, because inventing advice for those would be worse than silence.
  plugin_not_installed: 'That device does not have this plugin.',
  not_paired: 'These devices are not paired yet.',
  transport_not_open: 'The connection is not ready; try again in a moment.',
  transfer_failed: 'The transfer did not finish.',
  integrity_mismatch: 'The file arrived damaged, so it was not used.',
  payload_too_large: 'That is too large to send this way.',
  rate_limited: 'Too many requests just now; wait a moment and try again.',
  // The message-key half covers `error.timeout`, but the code half reaches here when a timeout arrives without that key.
  timeout: 'That took too long and was stopped.',
  // The code half of the key above, for a refusal whose message key this table has not met.
  unsupported: 'This version cannot do that yet.',
  unknown: 'That did not work.'
} as const

/** The labels for the kernel's suggested actions, when it sends one with an error. */
export const actions = {
  openSettings: 'Open settings',
  requestCapability: 'Ask for the permission',
  installPlugin: 'Open plugins',
  connectDevice: 'Open devices',
  retry: 'Try again',
  viewLogs: 'View the log'
} as const
