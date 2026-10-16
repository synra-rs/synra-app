/**
 * The two factories every client test is built from.
 *
 * # Why this is not a `.test.ts`
 *
 * The runner collects `src/**\/*.test.ts` and treats each collected file as a suite. A helper module that is collected
 * runs its own empty suite and, worse, reads as a file of tests that asserts nothing. It is named for what it holds —
 * fixtures and factories — so nobody goes looking for assertions in it.
 */

import type { ErrorCode } from '@synra/protocol'
import type { Bridge, IpcError } from './client'

/** A bridge that records its calls and answers with what a test queued. */
export function bridge(answer: unknown = { ok: true }): {
  bridge: Bridge
  commands: string[]
  listeners: Map<string, (payload: unknown) => void>
} {
  const commands: string[] = []
  const listeners = new Map<string, (payload: unknown) => void>()
  return {
    commands,
    listeners,
    bridge: {
      invoke: (command) => {
        commands.push(command)
        return Promise.resolve(answer)
      },
      listen: (event, handler) => {
        listeners.set(event, handler)
        return Promise.resolve(() => listeners.delete(event))
      }
    }
  }
}

/** An error with the three fields the interface reads. */
export function ipcError(code: ErrorCode, retryable: boolean): IpcError {
  return { code, message: 'error.someKey', retryable }
}
