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
  'error.badArgument': 'Something in the request was not valid.',
  'error.bridgeFailed': 'The interface could not reach the kernel.',
  'error.capabilityDenied': 'This needs a permission that has not been granted.',
  'error.commandNotFound': 'That is not something this version can do.',
  'error.storeUnavailable': 'The place that keeps this is not available.',
  'error.timeout': 'The kernel did not answer in time.'
} as const

export const errors = {
  capability_denied: 'This needs a permission that has not been granted.',
  device_offline: 'That device is not answering.',
  invalid_argument: 'Something in the request was not valid.',
  not_found: 'That is not here any more.',
  plugin_faulted: 'That plugin stopped working.',
  unauthorized: 'This is not allowed.',
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
