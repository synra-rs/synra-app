// @vitest-environment jsdom
import { flushPromises } from '@vue/test-utils'
import { afterEach, describe, expect, it } from 'vitest'
import { localePreference, setLocalePreference } from '@/app/locale'
import { answer, IDENTITY, messages, mocks, mountPage, openCategory } from './settings.testkit'

describe('settings language preferences', () => {
  afterEach(() => setLocalePreference('system'))

  it('saves language through the kernel and updates existing captions and options', async () => {
    let saved = 'en'
    answer({ identity_get: IDENTITY, config_set: null })
    const fallback = mocks.call.getMockImplementation()!
    mocks.call.mockImplementation(async (command: string, args?: {
      patch?: {
        set?: Record<string, unknown>
      }
    }) => {
      if (command === 'config_get') {
        return {
          ok: true,
          value: {
            values: { 'ui.locale': saved },
            overridden: ['ui.locale']
          }
        }
      }
      if (command === 'config_set') {
        const next = args?.patch?.set?.['ui.locale']
        if (typeof next !== 'string')
          throw new TypeError('Missing locale preference')
        saved = next
        return { ok: true, value: null }
      }
      return fallback(command, args)
    })
    const wrapper = await mountPage()
    try {
      await flushPromises()
      const select = wrapper.findAllComponents({ name: 'ASelect' }).find(value => value.attributes('data-testid') === 'language-preference')!
      await select.vm.$emit('change', 'zh-CN')
      await flushPromises()
      expect(mocks.call).toHaveBeenCalledWith('config_set', {
        patch: {
          set: { 'ui.locale': 'zh-CN' }
        }
      })
      expect(wrapper.get('h1').text()).toBe('设置')
      expect(select.props('value')).toBe('zh-CN')
      await openCategory(wrapper, 'plugins')
      const policy = wrapper.findAllComponents({ name: 'ASelect' }).find(value => value.attributes('data-testid') === 'plugin-update-policy')!
      expect(policy.props('options')).toContainEqual({ value: 'never', label: '手动检查' })
    }
    finally {
      wrapper.unmount()
    }
  })

  it('keeps the current language when saving is refused', async () => {
    answer({
      config_get: {
        values: { 'ui.locale': 'en' },
        overridden: ['ui.locale']
      },
      identity_get: IDENTITY
    })
    const fallback = mocks.call.getMockImplementation()!
    mocks.call.mockImplementation((command: string, args?: unknown) => command === 'config_set'
      ? {
          ok: false,
          error: { code: 'capability_denied', message: 'error.capability_denied', retryable: false }
        }
      : fallback(command, args))
    const wrapper = await mountPage()
    try {
      await flushPromises()
      const select = wrapper.findAllComponents({ name: 'ASelect' }).find(value => value.attributes('data-testid') === 'language-preference')!
      await select.vm.$emit('change', 'zh-CN')
      await flushPromises()
      expect(localePreference.value).toBe('en')
      expect(wrapper.get('h1').text()).toBe('Settings')
      expect(wrapper.text()).toContain(messages.errors.capability_denied)
    }
    finally {
      wrapper.unmount()
    }
  })
})
