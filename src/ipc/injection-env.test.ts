/**
 * What the host says about itself, and what the three "can"s mean.
 *
 * # Why the two are one file
 *
 * Both are the same question asked of `env`: what a plugin is told about the machine it is running on, and which of the
 * three predicates answers about the platform, which about the grant and which about the host version. The library's own
 * comment warns that mixing the three makes a button's disabled condition wrong, so they are tested apart from each
 * other rather than apart from the facts around them.
 *
 * This is one of the groups the injected object is asserted in; the surface itself is in `injection-surface.test.ts` and
 * the shared factory is in `injection-test-helpers.ts`, which the runner does not collect.
 */

import { describe, expect, it } from 'vitest'
import { createInjection } from './injection'
import { context } from './injection-test-helpers'

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
  it('can answers about the grant', () => {
    const synra = createInjection(context({ granted: ['platform:read-clipboard'] }))
    expect(synra.env.can('platform:read-clipboard')).toBe(true)
    expect(synra.env.can('platform:write-clipboard')).toBe(false)
  })

  it('platformSupports answers about the platform, never the grant', () => {
    // ★ Three contexts, and the grant is deliberately the opposite of what a reader might expect: the one that was
    // granted and the one that was refused answer **the same**, and the one whose platform does not support it answers
    // false. A table derived from `granted` would flip the first two.
    const granted = createInjection(context({ granted: ['platform:read-clipboard'], platformCapabilities: ['clipboard_read'] }))
    const refused = createInjection(context({ granted: [], platformCapabilities: ['clipboard_read'] }))
    const elsewhere = createInjection(context({ platformCapabilities: [] }))

    expect(granted.env.platformSupports('clipboard_read')).toBe(true)
    expect(refused.env.platformSupports('clipboard_read')).toBe(true)
    expect(elsewhere.env.platformSupports('clipboard_read')).toBe(false)
    expect(elsewhere.env.platformSupports('notify')).toBe(false)
  })

  it('hostSupports answers about the host version', () => {
    const synra = createInjection(context())
    expect(synra.env.hostSupports('event.subscribe')).toBe(true)
    // ★ True since outbound requests went over the plugin channel. `false` here would be the same lie the table exists
    // to prevent: an author guarding against a failure the host no longer produces.
    expect(synra.env.hostSupports('network.fetch')).toBe(true)
    expect(synra.env.hostSupports('device.pick')).toBe(false)
  })
})
