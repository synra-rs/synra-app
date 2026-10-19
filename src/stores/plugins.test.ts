/**
 * The plugins store: the three things a card can do here.
 *
 * Appending, replacing in place, and being dropped. The replacement case is the one worth asserting — it is what makes an
 * enabled plugin update without the list reshuffling — and the `false` case is the contract every store in this app
 * shares.
 */

import type { PluginSnapshot } from '@synra/protocol'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'

import { usePluginsStore } from './plugins'

function card(pluginId: string, overrides: Partial<PluginSnapshot> = {}): PluginSnapshot {
  return {
    pluginId,
    title: pluginId,
    version: '1.0.0',
    publisher: 'acme',
    enabled: false,
    state: 'awaiting_consent',
    hybrid: false,
    hasUi: false,
    granted: [],
    denied: [],
    requested: [],
    ...overrides
  } as PluginSnapshot
}

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('the plugins store', () => {
  it('takes the startup snapshot as the whole picture', () => {
    const store = usePluginsStore()

    store.applySnapshot([card('a'), card('b')])

    expect(store.plugins.map(p => p.pluginId)).toEqual(['a', 'b'])
  })

  it('applies a card event, replacing in place', () => {
    const store = usePluginsStore()
    store.applySnapshot([card('a'), card('b')])

    expect(store.apply({ type: 'pluginUpdated', payload: card('b', { enabled: true }) })).toBe(true)
    expect(store.apply({ type: 'pluginUpdated', payload: card('c') })).toBe(true)

    expect(store.plugins.map(p => p.pluginId)).toEqual(['a', 'b', 'c'])
    expect(store.plugin('b')?.enabled).toBe(true)
  })

  it('refuses an event about something else', () => {
    const store = usePluginsStore()
    store.applySnapshot([card('a')])

    expect(store.apply({ type: 'deviceCardUpdated', payload: {} } as never)).toBe(false)
    expect(store.plugins).toHaveLength(1)
  })

  it('marks one card faulted when the ladder says so, and leaves the rest alone', () => {
    // ★ The event carries no card. Only the two fields the ladder decided change: the state word and its reason.
    const store = usePluginsStore()
    store.applySnapshot([card('a', { state: 'active' }), card('b', { state: 'active' })])

    expect(store.markHealth('a', 'faulted', 'it trapped five times')).toBe(true)

    expect(store.plugin('a')?.state).toBe('faulted')
    expect(store.plugin('a')?.stateReason).toBe('it trapped five times')
    expect(store.plugin('b')?.state).toBe('active')
  })

  it('forgets a card, which is what an uninstall leaves behind', () => {
    // The kernel has no removal event: a card for a plugin that no longer exists would be a card about nothing.
    const store = usePluginsStore()
    store.applySnapshot([card('a'), card('b')])

    store.forget('a')

    expect(store.plugins.map(p => p.pluginId)).toEqual(['b'])
  })
})
