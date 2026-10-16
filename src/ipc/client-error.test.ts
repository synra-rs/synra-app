/**
 * What counts as an error, and what the interface's own error type is.
 *
 * # Why the classification is tested apart from the calls
 *
 * `isIpcError` is the boundary between "the kernel answered" and "the kernel refused", and every caller's `if
 * (result.ok)` depends on it. Its two failure directions are opposite and both silent: a value accepted as an error is a
 * panel that shows a refusal for data, and a real error refused is a blank panel with nothing in the log.
 *
 * This file is the second of two: the calls themselves are in `client.test.ts`, and the factories both use are in
 * `client-test-helpers.ts`, which the runner does not collect.
 */

import type { IpcError } from './client'

import { describe, expect, it } from 'vitest'
import { isIpcError } from './client'
import { ipcError } from './client-test-helpers'

describe('whether a value is an error', () => {
  it('accepts the three fields the interface reads', () => {
    expect(isIpcError(ipcError('timeout', true))).toBe(true)
  })

  it('refuses anything missing one of them', () => {
    // Each field is used: `code` is what a caller branches on, `message` is the key the interface renders, `retryable`
    // is whether to offer the button. One missing is one the interface cannot render correctly.
    expect(isIpcError({ code: 'timeout', retryable: true })).toBe(false)
    expect(isIpcError({ code: 'timeout', message: 'k' })).toBe(false)
    expect(isIpcError({ message: 'k', retryable: true })).toBe(false)
    expect(isIpcError({ code: 'timeout', message: 'k', retryable: 'yes' })).toBe(false)
  })

  it('refuses the things a bridge might send instead', () => {
    // A string, a number, null and a bare Error are all shapes a WebView bridge can produce for a failure, and none of
    // them is renderable — so each becomes `internal` through `bridgeFailure` rather than being read as a code.
    for (const value of ['a string', 42, null, undefined, [], new Error('boom')]) {
      expect(isIpcError(value)).toBe(false)
    }
  })

  it('accepts a richer error, because more fields is not less information', () => {
    // The Rust type has `params`, `details` and `actions`, and a check that required an exact field set would refuse
    // the real error the moment the kernel gained one.
    expect(
      isIpcError({
        code: 'invalid_argument',
        message: 'error.badArgument',
        params: { field: 'name' },
        details: 'the name was empty',
        retryable: false,
        actions: ['openSettings']
      })
    ).toBe(true)
  })
})

describe('the error type the interface uses', () => {
  it('is not the plugin-facing one', () => {
    // A compile-time assertion with a runtime witness: `message` here is a **key** and in `SynraError` a sentence, and
    // the field the interface needs to localise is the field a plugin cannot use. Two types rather than one is what lets
    // the interface render in the user's language.
    const error: IpcError = { code: 'timeout', message: 'error.timeout', retryable: true }
    expect(error.message).toMatch(/^error\./)
  })
})
