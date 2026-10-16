/**
 * The injected object, asserted against the contract it claims to implement.
 *
 * # What is being tested, and why it is written the way it is
 *
 * `satisfies SynraClient` in the source is what enforces the shape at compile time; these tests are what make the
 * enforcement *visible* to someone who reads the test output and never opens the type checker. They also pin the
 * members the design document listed — `isCapabilityDenied` and `ready` — which the implementation did not have, and
 * the two facts a plugin branches on that used to be hardcoded constants.
 *
 * The behaviour groups are separate files beside this one (`injection-env.test.ts` for what the host says about itself
 * and the three "can"s, `injection-capabilities.test.ts` for the advertised/actual biconditional,
 * `injection-refusals.test.ts` for the refusals and the subscription guarantees), and the factory they all share is in
 * `injection-test-helpers.ts`, which the runner does not collect.
 */

import type { ErrorCode } from '@synra/protocol'
import { describe, expect, it, vi } from 'vitest'
import { createInjection } from './injection'
import { context } from './injection-test-helpers'

describe('the injected object', () => {
  it('has every namespace and no more', () => {
    // ★ The reason this file is written the way it is.
    //
    // `satisfies` in the source is what enforces this at compile time; this test is what makes the enforcement
    // *visible* to someone who reads the test output and never opens the type checker.
    const synra = createInjection(context())
    expect(Object.keys(synra).sort()).toEqual([
      'clipboard',
      'device',
      'env',
      'event',
      'isCapabilityDenied',
      'isCode',
      'isSynraError',
      'log',
      'network',
      'platform',
      'ready',
      'requestId',
      'self',
      'storage',
      'transfer'
    ])
  })
})

describe('the two members the design listed and the implementation did not have', () => {
  // ★ Found by compiling the design document's complete authoring example (`synra-js` guard 8), not by reading either
  // side. The document's §4.1 lists `isCapabilityDenied` and `ready`, both of its examples use `isCapabilityDenied`, and
  // the injected object had neither — so an author copying the example got a method that does not exist.
  it('answers isCapabilityDenied by narrowing, not by a boolean', () => {
    const synra = createInjection(context())
    const denied = { code: 'capability_denied', message: 'no', params: {} }

    expect(synra.isCapabilityDenied(denied)).toBe(true)
    // A different code is not a capability denial, which is the whole reason the specific predicate exists.
    expect(synra.isCapabilityDenied({ ...denied, code: 'unsupported' })).toBe(false)
    // And the same runtime check as `isSynraError` underneath: a string is not an error however it is shaped.
    expect(synra.isCapabilityDenied('capability_denied')).toBe(false)
    expect(synra.isCapabilityDenied(undefined)).toBe(false)
  })

  it('tells the host the page is ready, through the contract\'s own command', () => {
    // The command name is asserted rather than mocked away: `plugin_ui_ready` is one of the kernel's declared commands,
    // and the app's `command-names` shape check verifies every name called here against that table. A test that stubbed
    // the bridge would not have caught an invented name.
    const calls: string[] = []
    const synra = createInjection(
      context({
        bridge: {
          invoke: (command) => {
            calls.push(command)
            return Promise.resolve(undefined)
          },
          listen: () => Promise.resolve(() => undefined)
        }
      })
    )

    synra.ready()

    expect(calls).toEqual(['plugin_ui_ready'])
  })

  it('does not let a host that has not registered the command break the page', () => {
    // ★ `ready(): void` is synchronous and the page does not wait. A shell that does not know the command yet must not
    // turn a plugin author's console into an unhandled rejection naming something they never called.
    const synra = createInjection(
      context({
        bridge: {
          invoke: () => Promise.reject(new Error('command plugin_ui_ready not found')),
          listen: () => Promise.resolve(() => undefined)
        }
      })
    )

    expect(() => synra.ready()).not.toThrow()
  })
})

describe('error narrowing', () => {
  it('recognises its own error and rejects an impostor', () => {
    const synra = createInjection(context())
    const real = { code: 'capability_denied', message: 'error.capabilityDenied', retryable: false }
    expect(synra.isSynraError(real)).toBe(true)
    expect(synra.isCode(real, 'capability_denied')).toBe(true)
    expect(synra.isCode(real, 'timeout' as ErrorCode)).toBe(false)
    expect(synra.isSynraError({ code: 1 })).toBe(false)
    expect(synra.isSynraError(undefined)).toBe(false)
    expect(synra.isSynraError('capability_denied')).toBe(false)
  })
})

describe('requestId', () => {
  // ★ From the host, and distinct per call, because the identifier goes into the kernel's de-duplication table.
  it('is unique per call and carries the plugin', () => {
    const synra = createInjection(context())
    const first = synra.requestId()
    const second = synra.requestId()
    expect(first).not.toBe(second)
    expect(first).toContain('com.example.notes')
  })
})

describe('logging', () => {
  it('prefixes a child logger and nests it', () => {
    const lines: string[] = []
    const spy = vi.spyOn(console, 'info').mockImplementation((line: unknown) => {
      lines.push(String(line))
      return undefined
    })
    try {
      const synra = createInjection(context())
      synra.log.child('net').info('connected')
      synra.log.child('net').child('retry').info('again')
      expect(lines).toEqual(['net connected', 'net.retry again'])
    }
    finally {
      spy.mockRestore()
    }
  })
})
