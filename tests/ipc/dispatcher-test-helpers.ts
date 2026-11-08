/**
 * The two factories every dispatcher test is built from.
 *
 * # Why this is not a `.test.ts`
 *
 * The runner collects `src/**\/*.test.ts` and treats each collected file as a suite. A helper module that is collected
 * runs its own empty suite and, worse, reads as a file of tests that asserts nothing. It is named for what it holds —
 * fixtures and factories — so nobody goes looking for assertions in it.
 */

import type { SequencedKernelEvent } from '@synra/protocol'
import { vi } from 'vitest'

/**
 * A refresh callback that records why it was called.
 *
 * A `vi.fn` rather than a plain closure, so a test can assert that it was **not** called — `not.toHaveBeenCalled` needs
 * a spy, and "no snapshot was asked for" is the assertion half of these tests make.
 */
export function refresher(): {
  refresh: ReturnType<typeof vi.fn<(reason: string) => void>>
  reasons: () => string[]
} {
  const refresh = vi.fn<(reason: string) => void>()
  return { refresh, reasons: () => refresh.mock.calls.map(([reason]) => reason) }
}

/** A log event with a sequence number. */
export function logLine(seq: number, line = 'x'): SequencedKernelEvent {
  return { seq, at: seq, type: 'logLine', payload: { level: 'info', line } }
}

/** A device card event with a sequence number. */
export function deviceUpdated(seq: number, deviceId = 'dev_a'): SequencedKernelEvent {
  return {
    seq,
    at: seq,
    type: 'deviceCardUpdated',
    payload: {
      deviceId,
      displayName: 'Laptop',
      platform: 'windows',
      online: true,
      paired: true,
      blocked: false,
      lastSeenMs: seq
    }
  }
}
