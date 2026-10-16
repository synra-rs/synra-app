/**
 * The two factories every injection test is built from.
 *
 * # Why this is not a `.test.ts`
 *
 * The runner collects `src/**\/*.test.ts` and treats each collected file as a suite. A helper module that is collected
 * runs its own empty suite and, worse, reads as a file of tests that asserts nothing. It is named for what it holds —
 * fixtures and factories — so nobody goes looking for assertions in it.
 */

import type { ClientFeature, SynraClient } from '@synra/client'
import type { Bridge } from './client'

import type { InjectionContext } from './injection'

import { vi } from 'vitest'

/**
 * A bridge that answers nothing and records what it was asked.
 *
 * Every test needs a different answer, so the ones that care about a specific command replace `invoke`; the ones that
 * care about refusals leave this one.
 */
export function bridge(overrides: Partial<Bridge> = {}): Bridge {
  return {
    invoke: vi.fn(async () => ({})),
    listen: vi.fn(async () => () => {
      // A subscription that can be dropped.
    }),
    ...overrides
  }
}

export function context(overrides: Partial<InjectionContext> = {}): InjectionContext {
  return {
    bridge: bridge(),
    pluginId: 'com.example.notes',
    granted: [],
    deniedCapabilities: [],
    locale: 'zh-CN',
    darkMode: false,
    sdkVersion: '0.1.0',
    // ★ The platform is a parameter now rather than a constant, so every test names the machine it is testing on.
    // The default is deliberately *not* `'windows'`: that is the value the facade used to return for every platform,
    // and a default of `'windows'` would let the tests keep passing if it ever went back to being hardcoded.
    platform: 'linux',
    hostKind: 'desktop',
    appVersion: '0.1.0',
    ...overrides
  }
}

/** The call a feature name stands for, so the test can ask the object to do the thing. */
export const CALLS: Readonly<Record<ClientFeature, (synra: SynraClient) => Promise<unknown>>> = {
  'event.subscribe': async (synra) => {
    synra.event.subscribe('anything', () => {
      // Nothing is done with it; the point is that subscribing resolves.
    })
  },
  'device.onChange': async (synra) => {
    synra.device.onChange(() => {
      // Nothing is done with it; the point is that subscribing resolves.
    })
  },
  'device.pick': async synra => synra.device.pick(),
  'transfer.onProgress': async (synra) => {
    // ★ The id comes first. A call written the other way round typechecks against a one-argument signature and then
    // subscribes nothing — the failure this file exists to prevent, in the test for the guard itself.
    synra.transfer.onProgress('transfer-1', () => {
      // Nothing is done with it.
    })
  },
  'network.fetch': async synra => synra.network.fetch('https://example.invalid'),
  'clipboard.read': async synra => synra.clipboard.read()
}

/** Every feature the interface declares, so a new one cannot be added without a test noticing. */
export const FEATURES: readonly ClientFeature[] = [
  'device.onChange',
  'device.pick',
  'transfer.onProgress',
  'event.subscribe',
  'network.fetch',
  'clipboard.read'
]
