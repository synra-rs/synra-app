/**
 * Every kernel error code that a person can act on has its own sentence.
 *
 * ★ [04 §6](../../design-docs/ui-design/04-interaction-patterns.md) asks a failure to carry "a cause plus an executable
 * recovery step", and explicitly rules out leaving the reader with a generic apology. `ErrorNotice` looks a message key
 * up first and the code second, then falls back to `errors.unknown` — "That did not work." — so a code missing from this
 * table is a code whose users get told nothing. Before this file existed, 14 of the kernel's 22 codes fell through that
 * way, including three a person meets routinely: not paired, plugin not installed, payload too large.
 */
import { describe, expect, it } from 'vitest'

import { actions, errors } from './messages/errors'

/**
 * The codes `synra_protocol::ErrorCode` serializes, minus the ones the interface has nothing honest to say about yet.
 *
 * The excluded set is deliberate, not forgotten: a plugin sandbox that trapped or ran out of fuel, a protocol version
 * mismatch, an outbound host/IP/scheme the allowlist refused. Those describe conditions the person on this screen cannot
 * change, and inventing advice for them would be worse than the shared fallback. If one of them ever becomes something a
 * user can act on, it moves out of this list and gains a sentence.
 */
const ACTIONABLE_KERNEL_CODES = [
  'capability_denied',
  'device_offline',
  'invalid_argument',
  'not_paired',
  'payload_too_large',
  'plugin_not_installed',
  'rate_limited',
  'timeout',
  'transfer_failed',
  'integrity_mismatch',
  'transport_not_open'
] as const

describe('error sentences', () => {
  it.each(ACTIONABLE_KERNEL_CODES)('has a sentence for the kernel code %s', (code) => {
    expect(errors[code as keyof typeof errors]).toBeTypeOf('string')
    expect(errors[code as keyof typeof errors].length).toBeGreaterThan(0)
  })

  it('keeps the fallback out of the actionable set, so no code above silently reads as it', () => {
    // A code that maps to the same words as `unknown` is a code that gained an entry without gaining a sentence —
    // the shape the mistake took the first time.
    for (const code of ACTIONABLE_KERNEL_CODES) {
      expect(errors[code as keyof typeof errors]).not.toBe(errors.unknown)
    }
  })
})

/**
 * Every `SuggestedAction` variant has a label.
 *
 * ★ `ErrorNotice` renders `messages.actions[action.kind]` for all three of its branches — a routed button, the retry
 * button, and the plain-text direction. When the kernel grows a seventh variant and this table does not, that
 * expression is `undefined`: the notice draws a button with no words on it, next to an error explaining itself. The
 * table has no fallback, so nothing else would catch it.
 *
 * These are the six `synra_protocol::SuggestedAction` variants. The enum is `#[non_exhaustive]`, which is exactly why
 * the list is written here rather than derived: it is a reminder to update this file, not a claim it never changes.
 */
const SUGGESTED_ACTION_KINDS = [
  'openSettings',
  'requestCapability',
  'installPlugin',
  'connectDevice',
  'retry',
  'viewLogs'
] as const

describe('suggested action labels', () => {
  it.each(SUGGESTED_ACTION_KINDS)('has a label for the kernel action %s', (kind) => {
    expect(actions[kind]).toBeTypeOf('string')
    expect(actions[kind].length).toBeGreaterThan(0)
  })

  it('declares no label the kernel cannot send', () => {
    // The other direction: a label with no variant behind it is dead weight that looks like support.
    expect(Object.keys(actions).sort()).toEqual([...SUGGESTED_ACTION_KINDS].sort())
  })
})
