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
 * The behaviour groups are separate files beside this one (`injection-capabilities.test.ts`,
 * `injection-refusals.test.ts`), and the factories they all share are in `injection-test-helpers.ts`, which the runner
 * does not collect.
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

describe('what the host says about itself', () => {
  // ★ These four used to be literals — `'desktop'`, `'windows'`, `'0.1.0'` — returned for every machine, so the
  // interface told a plugin on Linux, macOS, Android and iOS that it was running on Windows. A plugin that branched on
  // `platform()` took the Windows path everywhere, and nothing failed, because a plausible wrong answer is
  // indistinguishable from a right one at the call site.
  //
  // The test that would have caught it is this one: ask on two different platforms and require two different answers.
  // A single assertion against `'windows'` would have passed happily on the one machine CI ran on.
  it('reports the platform it was given, not a hardcoded one', () => {
    const onLinux = createInjection(context({ platform: 'linux' }))
    const onMac = createInjection(context({ platform: 'macos' }))
    const onAndroid = createInjection(context({ platform: 'android' }))

    expect(onLinux.env.platform()).toBe('linux')
    expect(onMac.env.platform()).toBe('macos')
    expect(onAndroid.env.platform()).toBe('android')
    // ★ And they differ, which is the part that makes the three assertions above a test rather than a transcription of
    // the implementation. A facade that ignored the context and returned one value would fail here.
    expect(new Set([onLinux.env.platform(), onMac.env.platform(), onAndroid.env.platform()]).size).toBe(3)
  })

  it('reports the host kind and the app version it was given', () => {
    const synra = createInjection(context({ hostKind: 'mobile', appVersion: '9.9.9' }))

    expect(synra.env.hostKind()).toBe('mobile')
    expect(synra.env.appVersion()).toBe('9.9.9')
  })

  it('keeps the host kind and the SDK version apart', () => {
    // Two different versions, and the point is that neither is the other: a plugin asking for the SDK version must not
    // be told the application's, which is what a single hardcoded string would have done.
    const synra = createInjection(context({ appVersion: '9.9.9', sdkVersion: '1.2.3' }))

    expect(synra.env.appVersion()).toBe('9.9.9')
    expect(synra.env.sdkVersion()).toBe('1.2.3')
  })
})

describe('the three "can"s mean three different things', () => {
  // ★ The library's own comment warns that mixing these makes a button's disabled condition wrong. So they are tested
  // apart: a capability the platform cannot do but the user granted is still granted, and asking about the grant must
  // not be answered by the platform.
  it('can answers about the grant', () => {
    const synra = createInjection(context({ granted: ['clipboard_read'] }))
    expect(synra.env.can('clipboard_read')).toBe(true)
    expect(synra.env.can('clipboard_write')).toBe(false)
  })

  it('platformSupports answers about the platform, never the grant', () => {
    const granted = createInjection(context({ granted: ['clipboard_read'] }))
    const refused = createInjection(context({ granted: [] }))
    expect(granted.env.platformSupports('clipboard_read'))
      .toBe(refused.env.platformSupports('clipboard_read'))
  })

  it('hostSupports answers about the host version', () => {
    const synra = createInjection(context())
    expect(synra.env.hostSupports('event.subscribe')).toBe(true)
    expect(synra.env.hostSupports('network.fetch')).toBe(false)
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
