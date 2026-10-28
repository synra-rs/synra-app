/**
 * The settings store: three claims.
 *
 * A snapshot replaces rather than merges (a key the kernel no longer lists is one nobody overrode any more), an event
 * carrying the configuration is applied, and an event about something else is refused so its caller can re-read.
 */

import type { ConfigSnapshot } from '@synra/protocol'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { localePreference, setLocalePreference } from '../locale'
import { useSettingsStore } from './settings'

function snapshot(values: Record<string, unknown>): ConfigSnapshot {
  return { values, overridden: Object.keys(values) } as ConfigSnapshot
}

beforeEach(() => {
  setActivePinia(createPinia())
})

afterEach(() => setLocalePreference('system'))

it('restores the saved language and follows authoritative configuration events', async () => {
  const store = useSettingsStore()
  const call = vi.fn().mockResolvedValue({ ok: true, value: snapshot({ 'ui.locale': 'zh-CN' }) })
  await store.read({ call })
  expect(localePreference.value).toBe('zh-CN')
  store.apply({ type: 'configUpdated', payload: snapshot({ 'ui.locale': 'en' }) })
  expect(localePreference.value).toBe('en')
  store.applySnapshot(snapshot({}))
  expect(localePreference.value).toBe('system')
})

it('does not let a late startup read overwrite a newer language event', async () => {
  const store = useSettingsStore()
  let finish!: (value: unknown) => void
  const call = vi.fn().mockImplementation(() => new Promise((resolve) => {
    finish = resolve
  }))
  const pending = store.read({ call })
  store.apply({ type: 'configUpdated', payload: snapshot({ 'ui.locale': 'zh-CN' }) })
  finish({ ok: true, value: snapshot({ 'ui.locale': 'en' }) })
  await pending
  expect(store.value('ui.locale')).toBe('zh-CN')
  expect(localePreference.value).toBe('zh-CN')
})

it('keeps the latest read when responses arrive out of order', async () => {
  const store = useSettingsStore()
  let finish!: (value: unknown) => void
  const call = vi.fn().mockImplementationOnce(() => new Promise((resolve) => {
    finish = resolve
  })).mockResolvedValueOnce({ ok: true, value: snapshot({ 'ui.locale': 'zh-CN' }) })
  const older = store.read({ call })
  await store.read({ call })
  finish({ ok: true, value: snapshot({ 'ui.locale': 'en' }) })
  await older
  expect(localePreference.value).toBe('zh-CN')
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
