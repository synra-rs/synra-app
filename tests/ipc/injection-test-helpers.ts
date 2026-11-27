/**
 * Fixtures and factories for the plugin facade and its event stream.
 *
 * # Why this is not a `.test.ts`
 *
 * The runner collects `src/**\/*.test.ts` and treats each collected file as a suite. A helper module that is collected
 * runs its own empty suite and, worse, reads as a file of tests that asserts nothing. It is named for what it holds —
 * fixtures and factories — so nobody goes looking for assertions in it.
 */

import type { ClientFeature, SynraClient } from '@synra/client'
import type { Bridge } from '@/ipc/client'

import type { InjectionContext } from '@/ipc/injection'

import { expect, vi } from 'vitest'
import eventFixture from '../fixtures/plugin-event-contract.json'

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
    // A label the host would have made for a page: it is what plugin_ui_ready is called with.
    label: 'plugin-com.example.notes',
    // The token the same page was issued, in the decimal form `plugin_ui_rpc` accepts.
    token: '123456789012345678901234567890',
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
    // ★ Empty by default: a test that cares about `env.platformSupports` passes the list it is asserting about, and a
    // default of "everything" would make the tests pass on a host that supports nothing.
    platformCapabilities: [],
    ...overrides
  }
}

export function streamFixture() {
  const reads: { resolve: (value: unknown) => void, reject: (reason: unknown) => void }[] = []
  let overlapping = false
  const refresh = vi.fn<() => unknown>(() => ({ seq: 0, events: [] }))
  const status = vi.fn<() => unknown>(() => eventFixture.transfer.payload)
  const invoke = vi.fn(async (command: string, request?: Readonly<Record<string, unknown>>) => {
    expect(command).toBe('plugin_ui_rpc')
    switch (request?.method) {
      case 'event.subscribe': return 'stream'
      case 'event.poll': return new Promise((resolve, reject) => {
        if (reads.length !== 0)
          overlapping = true
        reads.push({ resolve, reject })
      })
      case 'event.resync': return refresh()
      case 'event.unsubscribe': return null
      case 'device.list': return eventFixture.envelope.payload.devices
      case 'transfer.status': return status()
      default: throw new Error('unexpected event command')
    }
  })
  const transport = bridge({ invoke })
  return {
    transport,
    invoke,
    refresh,
    status,
    async push(frame: unknown) {
      await vi.waitFor(() => expect(reads).toHaveLength(1))
      const read = reads.shift()
      read?.resolve(frame)
    },
    async fail(error: unknown) {
      await vi.waitFor(() => expect(reads).toHaveLength(1))
      reads.shift()?.reject(error)
    },
    isOverlapping: () => overlapping
  }
}

/** The call a feature name stands for, so the test can ask the object to do the thing. */
export const CALLS: Readonly<Record<ClientFeature, (synra: SynraClient) => Promise<unknown>>> = {
  'event.subscribe': async (synra) => {
    const off = synra.event.subscribe('anything', () => {
      // Nothing is done with it; the point is that subscribing resolves.
    })
    off()
  },
  'device.onChange': async (synra) => {
    const off = synra.device.onChange(() => {
      // Nothing is done with it; the point is that subscribing resolves.
    })
    off()
  },
  'device.pick': async synra => synra.device.pick(),
  'transfer.onProgress': async (synra) => {
    // ★ The id comes first. A call written the other way round typechecks against a one-argument signature and then
    // subscribes nothing — the failure this file exists to prevent, in the test for the guard itself.
    const off = synra.transfer.onProgress('transfer-1', () => {
      // Nothing is done with it.
    })
    off()
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
