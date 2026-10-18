// @vitest-environment jsdom

/**
 * The façade bootstrap: what a plugin page has before its own scripts run.
 *
 * The seed is the shell's JSON, with every field decided before the page loaded. The tests hand `inject` a fake bridge,
 * because the real one loads Tauri's API and a jsdom test has no way to answer it — and because the point here is the
 * context, not the transport.
 */

import type { InjectionSeed } from './injection/bootstrap'

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { bridge } from './injection-test-helpers'
import { boot, inject } from './injection/bootstrap'

function seed(overrides: Partial<InjectionSeed> = {}): InjectionSeed {
  return {
    label: 'plugin-com.example.notes',
    token: '123456789012345678901234567890',
    pluginId: 'com.example.notes',
    granted: ['log:*'],
    deniedCapabilities: ['network:fetch'],
    locale: 'zh-CN',
    darkMode: true,
    sdkVersion: '0.1.0',
    platform: 'linux',
    hostKind: 'desktop',
    appVersion: '9.9.9',
    ...overrides
  }
}

beforeEach(() => {
  delete (window as unknown as { synra?: unknown }).synra
})

describe('injecting the façade into a plugin page', () => {
  it('publishes the client on window.synra', () => {
    const client = inject(seed(), bridge())

    expect(window.synra).toBe(client)
    expect(client.env.platform()).toBe('linux')
  })

  it('boots the entry the initialization script calls', () => {
    // ★ `boot` is what `SynraInjection.boot(seed)` resolves to in the page. It constructs the Tauri bridge, which loads
    // its API lazily — so this call builds the façade and touches nothing that jsdom cannot answer.
    const client = boot(seed())

    expect(window.synra).toBe(client)
    expect(client.env.hostKind()).toBe('desktop')
  })

  it('answers the environment from the seed rather than from the machine', () => {
    // ★ The failure this guards is the one the façade used to have: `platform()` returned the hardcoded `'windows'` on
    // every machine. These answers are the host's, and a test machine must not leak into them.
    const client = inject(seed({ platform: 'android', hostKind: 'mobile', darkMode: false, locale: 'en' }), bridge())

    expect(client.env.platform()).toBe('android')
    expect(client.env.hostKind()).toBe('mobile')
    expect(client.env.darkMode()).toBe(false)
    expect(client.env.locale()).toBe('en')
    expect(client.env.sdkVersion()).toBe('0.1.0')
    expect(client.env.appVersion()).toBe('9.9.9')
  })

  it('knows what the user allowed and what they refused', () => {
    const client = inject(seed(), bridge())

    expect(client.env.can('log:*')).toBe(true)
    expect(client.env.can('network:fetch')).toBe(false)
  })

  it('calls the plugin channel with the token the host issued', async () => {
    // ★ The end of the chain that the shell starts: the token from the seed is the one `plugin_ui_rpc` validates, and
    // device.list goes through it rather than through a host command.
    const invoke = vi.fn(async () => [])
    const client = inject(seed(), bridge({ invoke }))

    await expect(client.device.list()).resolves.toEqual([])
    expect(invoke).toHaveBeenCalledWith('plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'device.list',
      args: {}
    })
  })

  it('keeps a refusal a refusal', async () => {
    const invoke = vi.fn(async () => {
      throw Object.assign(new Error('no session'), { code: 'capability_denied', message: 'error.capabilityDenied' })
    })
    const client = inject(seed(), bridge({ invoke }))

    await expect(client.device.list()).rejects.toMatchObject({ code: 'capability_denied' })
  })
})
