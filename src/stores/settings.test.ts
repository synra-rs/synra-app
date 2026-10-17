/**
 * The settings store: three claims.
 *
 * A snapshot replaces rather than merges (a key the kernel no longer lists is one nobody overrode any more), an event
 * carrying the configuration is applied, and an event about something else is refused so its caller can re-read.
 */

import type { ConfigSnapshot } from '@synra/protocol'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'

import { useSettingsStore } from './settings'

function snapshot(values: Record<string, unknown>): ConfigSnapshot {
  return { values, overridden: Object.keys(values) } as ConfigSnapshot
}

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('the settings store', () => {
  it('takes a snapshot as the whole picture', () => {
    const store = useSettingsStore()

    store.applySnapshot(snapshot({ 'ui.locale': 'zh', 'ui.theme': 'dark' }))

    expect(store.value('ui.locale')).toBe('zh')
    expect(store.overridden).toEqual(['ui.locale', 'ui.theme'])
  })

  it('replaces rather than merges, so a removed override disappears', () => {
    // ★ The failure this prevents: a user resets a setting in one window, and another window keeps showing it as
    // overridden because merging kept the key nobody had any more.
    const store = useSettingsStore()
    store.applySnapshot(snapshot({ 'ui.locale': 'zh' }))

    store.applySnapshot(snapshot({}))

    expect(store.value('ui.locale')).toBeUndefined()
    expect(store.overridden).toEqual([])
  })

  it('applies a configuration event', () => {
    const store = useSettingsStore()

    expect(store.apply({ type: 'configUpdated', payload: snapshot({ 'ui.locale': 'en' }) })).toBe(true)

    expect(store.value('ui.locale')).toBe('en')
  })

  it('refuses an event about something else', () => {
    const store = useSettingsStore()

    expect(store.apply({ type: 'deviceCardUpdated', payload: {} } as never)).toBe(false)
  })
})
