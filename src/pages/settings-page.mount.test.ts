import { flushPromises } from '@vue/test-utils'
// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest'
import { buildStamp, coreBuildStamp } from '../build-stamp'
import { localePreference, setLocalePreference } from '../locale'
import { answer, IDENTITY, messages, mocks, mountPage, NO_METRICS, openCategory } from './settings-page.testkit'

describe('the settings page', () => {
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
      if (command === 'config_get')
        return { ok: true, value: { values: { 'ui.locale': saved }, overridden: ['ui.locale'] } }
      if (command === 'config_set') {
        const next = args?.patch?.set?.['ui.locale']
        if (typeof next !== 'string')
          throw new Error('Missing locale preference')
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
      expect(mocks.call).toHaveBeenCalledWith('config_set', { patch: { set: { 'ui.locale': 'zh-CN' } } })
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
  it('writes the wire console switch as a normal preference', async () => {
    let saved: unknown = true
    answer({ identity_get: IDENTITY, config_set: null })
    const fallback = mocks.call.getMockImplementation()!
    mocks.call.mockImplementation(async (command: string, args?: {
      patch?: {
        set?: Record<string, unknown>
      }
    }) => {
      if (command === 'config_get')
        return { ok: true, value: { values: { 'ui.debugConsole': saved }, overridden: ['ui.debugConsole'] } }
      if (command === 'config_set') {
        saved = args?.patch?.set?.['ui.debugConsole']
        return { ok: true, value: null }
      }
      return fallback(command, args)
    })
    const wrapper = await mountPage()
    try {
      await flushPromises()
      const wire = wrapper.findAllComponents({ name: 'ASwitch' }).find(value => value.attributes('aria-label') === messages.settings.wire.show)
      expect(wire).toBeDefined()
      expect(wire!.props('checked')).toBe(true)
      await wire!.vm.$emit('change', false)
      await flushPromises()
      expect(mocks.call).toHaveBeenCalledWith('config_set', { patch: { set: { 'ui.debugConsole': false } } })
    }
    finally {
      wrapper.unmount()
    }
  })
  it('keeps the current language when saving is refused', async () => {
    answer({ config_get: { values: { 'ui.locale': 'en' }, overridden: ['ui.locale'] }, identity_get: IDENTITY })
    const fallback = mocks.call.getMockImplementation()!
    mocks.call.mockImplementation((command: string, args?: unknown) => command === 'config_set'
      ? { ok: false, error: { code: 'capability_denied', message: 'error.capability_denied', retryable: false } }
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
  it('reads the configuration and this device when it is mounted', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })
    const wrapper = await mountPage()
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('config_get')
    expect(mocks.call).toHaveBeenCalledWith('identity_get')
    wrapper.unmount()
  })
  it('renames this device through the kernel and shows the name it read back', async () => {
    let reads = 0
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'diag_metrics')
        return { ok: true, value: NO_METRICS }
      if (command === 'config_get')
        return { ok: true, value: { values: {}, overridden: [] } }
      if (command === 'identity_get') {
        reads += 1
        return {
          ok: true,
          value: reads === 1 ? IDENTITY : { ...IDENTITY, displayName: 'Kitchen tablet', nameIsDefault: false }
        }
      }
      if (command === 'identity_set_display_name')
        return { ok: true, value: null }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })
    const wrapper = await mountPage()
    await flushPromises()
    const rename = wrapper.findAll('button').find(button => button.text() === messages.settings.field.rename)
    expect(rename, 'the rename button is missing').toBeDefined()
    await rename?.trigger('click')
    const input = wrapper.find('input#device-name')
    expect(input.exists(), 'the name input did not open').toBe(true)
    await input.setValue('Kitchen tablet')
    const save = wrapper.findAll('button').find(button => button.text() === messages.settings.field.save)
    expect(save, 'the save button is missing').toBeDefined()
    await save?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('identity_set_display_name', { name: 'Kitchen tablet' })
    expect(wrapper.text()).toContain('Kitchen tablet')
    wrapper.unmount()
  })
  it('shows a preference the kernel has a value for', async () => {
    answer({ config_get: { values: { 'ui.theme': 'dark' }, overridden: ['ui.theme'] }, identity_get: IDENTITY })
    const wrapper = await mountPage()
    await flushPromises()
    await openCategory(wrapper, 'about')
    expect(wrapper.text()).toContain(messages.settings.themeFixed)
    wrapper.unmount()
  })
  it('writes the cancel-confirmation preference under its own key', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY, config_set: null })
    const wrapper = await mountPage()
    await flushPromises()
    const switches = wrapper.findAllComponents({ name: 'ASwitch' })
    expect(switches).toHaveLength(4)
    const cancelSwitch = switches.find(value => value.attributes('aria-label') === messages.settings.transfers.cancelAsk)
    expect(cancelSwitch).toBeDefined()
    await cancelSwitch?.vm.$emit('change', false)
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: { set: { 'transfers.confirmCancel': false } }
    })
    wrapper.unmount()
  })
  it.each(['never', 'notify-only', 'auto-minor', 'auto-all'])('writes the selected %s update policy', async (policy) => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY, config_set: null })
    const wrapper = await mountPage()
    await flushPromises()
    await openCategory(wrapper, 'plugins')
    const select = wrapper.findAllComponents({ name: 'ASelect' }).find(select => select.attributes('data-testid') === 'plugin-update-policy')!
    expect(select.props('value')).toBe('notify-only')
    await select.vm.$emit('change', policy)
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: { set: { 'plugins.auto_update': policy } }
    })
    wrapper.unmount()
  })
  it('shows the stored update policy and reacts to another window changing it', async () => {
    answer({ config_get: { values: { 'plugins.auto_update': 'auto-minor' }, overridden: ['plugins.auto_update'] }, identity_get: IDENTITY })
    const wrapper = await mountPage()
    await flushPromises()
    await openCategory(wrapper, 'plugins')
    const select = wrapper.findAllComponents({ name: 'ASelect' }).find(select => select.attributes('data-testid') === 'plugin-update-policy')!
    expect(select.props('value')).toBe('auto-minor')
    const { useSettingsStore } = await import('../stores/settings')
    useSettingsStore().apply({ type: 'configUpdated', payload: { values: { 'plugins.auto_update': 'auto-all' }, overridden: ['plugins.auto_update'] } })
    await flushPromises()
    expect(select.props('value')).toBe('auto-all')
    wrapper.unmount()
  })
  it('writes the receive-notification switch under its own key', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY, config_set: null })
    const wrapper = await mountPage()
    await flushPromises()
    const notifySwitch = wrapper.findAllComponents({ name: 'ASwitch' }).find(value => value.attributes('aria-label') === messages.settings.transfers.notify)
    expect(notifySwitch).toBeDefined()
    await notifySwitch?.vm.$emit('change', false)
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: { set: { 'transfers.notifyOnReceive': false } }
    })
    wrapper.unmount()
  })
  it('writes the registry URL, and unsets it when the field is cleared', async () => {
    answer({
      config_get: { values: { 'plugins.registry': 'https://r.example' }, overridden: ['plugins.registry'] },
      identity_get: IDENTITY,
      config_set: null
    })
    const wrapper = await mountPage()
    await flushPromises()
    await openCategory(wrapper, 'plugins')
    const input = wrapper.find('input[placeholder="https://registry.example.com"]')
    expect((input.element as HTMLInputElement).value).toBe('https://r.example')
    await input.setValue('https://r2.example')
    const save = wrapper.findAll('button').find(button => button.text().includes('Save'))
    expect(save).toBeDefined()
    await save?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: { set: { 'plugins.registry': 'https://r2.example' } }
    })
    await input.setValue('')
    await save?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: { unset: ['plugins.registry'] }
    })
    wrapper.unmount()
  })
  it('shows frontend and core build identities under one Version section in About', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })
    const wrapper = await mountPage()
    await flushPromises()
    await openCategory(wrapper, 'about')
    expect(wrapper.text()).toContain(messages.settings.field.version)
    expect(wrapper.text()).toContain(messages.settings.field.frontend)
    expect(wrapper.text()).toContain(`v${buildStamp.version}(${buildStamp.shortHash || '—'})`)
    expect(wrapper.text()).toContain(messages.settings.field.core)
    expect(wrapper.text()).toContain(`v${coreBuildStamp.version}(${coreBuildStamp.shortHash || '—'})`)
    wrapper.unmount()
  })
})
