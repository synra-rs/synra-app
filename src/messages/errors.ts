/**
 * What the interface says when the kernel refuses something (R33).
 *
 * Not an apology and not a shrug: what happened, in the interface's own words. The `code` is kept for the diagnostics
 * page — the main interface does not print raw error strings (R34).
 *
 * ★ **One table.** The host's `IpcError.message` is a **key**, not a sentence — the interface is the side that renders
 * it (`src/ipc/client.ts` explains why). `errorSentence` maps that key through the same table as the code: a key is
 * `error.<reason>` with the reason in `snake_case` (`synra-ipc/src/error.rs`, `MessageKey`), so `error.timeout` and the
 * code `timeout` are the same sentence, and a key we have not met falls back to the code's, then to `unknown` — a
 * chain that always says *something* true.
 *
 * **A second table of `error.<camelCase>` keys used to live here.** Nothing in the kernel ever sent one: `MessageKey`
 * is built from the code's own `snake_case` name, and the eight keys had zero senders outside tests. It was deleted
 * rather than kept "in case" — a table nothing fills is a table nobody can test.
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

/**
 * The sentence for a refused call.
 *
 * ★ This used to be written out twice — once in `ErrorNotice` and once in `HomePage`'s intent row — with a comment in
 * the second saying it was "the same lookup `ErrorNotice` does". Same words, same lines, two places to keep in step.
 * Changing it in one copy and not the other would make one screen explain a failure differently from another, which is
 * worse than either answer.
 *
 * **It takes only the code now.** There used to be a second parameter for the kernel's message key, looked up in a
 * table this file no longer has (top of the file says why it went). The key is built from the code's own name, so the
 * lookup was `errors[code]` with extra steps.
 */
export function errorSentence(code: string): string {
  return errors[code as keyof typeof errors]
    ?? errors.unknown
}
