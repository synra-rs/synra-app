import { expect, it, vi } from 'vitest'
import { PreviewConfig } from './preview-config'

function storage() {
  const values = new Map<string, string>()
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value)
    }
  }
}

it('restores the selected language in a new browser preview and supports resetting it', () => {
  const disk = storage()
  const first = new PreviewConfig(disk)
  expect(first.apply({ set: { 'ui.locale': 'zh-CN' } })).toEqual({ values: { 'ui.locale': 'zh-CN' }, overridden: ['ui.locale'] })
  const reopened = new PreviewConfig(disk)
  expect(reopened.snapshot()).toEqual(first.snapshot())
  reopened.apply({ unset: ['ui.locale'] })
  expect(new PreviewConfig(disk).snapshot()).toEqual({ values: {}, overridden: [] })
})

it('reports a refused storage write without claiming the setting changed', () => {
  const config = new PreviewConfig({ getItem: () => null, setItem: vi.fn(() => {
    throw new Error('storage unavailable')
  }) })
  expect(config.apply({ set: { 'ui.locale': 'en' } })).toBe('unavailable')
  expect(config.snapshot()).toEqual({ values: {}, overridden: [] })
})

it('rejects malformed patches and tolerates corrupt preview storage', () => {
  const config = new PreviewConfig({ getItem: () => '{', setItem() {} })
  expect(config.apply({ set: [] })).toBe('invalid')
  expect(config.apply({ unset: [1] })).toBe('invalid')
  expect(config.apply(null)).toBe('invalid')
  expect(config.snapshot()).toEqual({ values: {}, overridden: [] })
})
