// @vitest-environment jsdom
import { flushPromises } from '@vue/test-utils'
import { Switch } from 'ant-design-vue'
import { describe, expect, it } from 'vitest'
import { answer, card, click, clickBody, ErrorNotice, messages, mocks, mountPage, PluginDetail, PluginRow, usePluginsStore } from './plugins/workflow.testkit'

describe('the plugin workspace', () => {
  it('renders several compact summaries without loading every detail', async () => {
    answer({ plugins_list: Array.from({ length: 5 }, (_, index) => card({ pluginId: `@acme/${index}`, title: `Reader ${index}` })) })
    const { wrapper } = await mountPage()
    expect(wrapper.findAllComponents(PluginRow)).toHaveLength(5)
    expect(wrapper.findComponent(PluginDetail).exists()).toBe(false)
    expect(mocks.call.mock.calls.map(([name]) => name)).toEqual(['plugins_list', 'plugins_publishers'])
  })
  it('opens a deep link without erasing the installed filter and returns focus', async () => {
    const { wrapper, router } = await mountPage()
    const filter = wrapper.get(`input[placeholder="${messages.plugins.filterPlaceholder}"]`)
    await filter.setValue('no-match')
    await router.replace('/plugins?plugin=%40acme%2Freader&other=keep')
    await flushPromises()
    expect(wrapper.getComponent(PluginDetail).text()).toContain('Reader')
    await wrapper.getComponent(PluginDetail).vm.$emit('close')
    await flushPromises()
    expect(router.currentRoute.value.query).toEqual({ other: 'keep' })
    expect((filter.element as HTMLInputElement).value).toBe('no-match')
    expect(document.activeElement).toBe(filter.element)
  })
  it('shows a missing object without substituting a different plugin', async () => {
    const { wrapper } = await mountPage('/plugins?plugin=missing')
    expect(wrapper.text()).toContain(messages.plugins.notFound('missing'))
    expect(wrapper.findComponent(PluginDetail).exists()).toBe(false)
  })
  it('keeps a failed switch beside the plugin that owns it', async () => {
    answer({ plugins_disable: () => ({ ok: false, error: { code: 'unavailable', message: 'error.unavailable' } }) })
    const { wrapper } = await mountPage()
    const row = wrapper.getComponent(PluginRow)
    await row.getComponent(Switch).vm.$emit('change', false)
    await flushPromises()
    expect(row.getComponent(ErrorNotice).props('code')).toBe('unavailable')
    expect(row.getComponent(Switch).props('checked')).toBe(true)
  })
  it('opens only plugins that declare an interface', async () => {
    answer({ plugin_ui_open: null, plugins_list: [card(), card({ pluginId: '@acme/headless', title: 'Headless', hasUi: false })] })
    const { wrapper } = await mountPage()
    await click(wrapper.getComponent(PluginRow), messages.plugins.openAction)
    expect(mocks.call).toHaveBeenCalledWith('plugin_ui_open', { pluginId: '@acme/reader' })
    expect(wrapper.findAllComponents(PluginRow)[1]!.text()).not.toContain(messages.plugins.openAction)
  })
  it('draws updates delivered by events without background update checks', async () => {
    const { wrapper } = await mountPage('/plugins?plugin=%40acme%2Freader')
    usePluginsStore().upsert(card({ update: { pluginId: '@acme/reader', version: '2.0.0', needsNewConsent: true, addedCapabilities: ['fs:read'] }, stateReason: 'Paused by the host' }))
    await flushPromises()
    const detail = wrapper.getComponent(PluginDetail)
    expect(detail.text()).toContain('2.0.0')
    expect(detail.text()).toContain('fs:read')
    expect(detail.text()).toContain('Paused by the host')
    expect(mocks.call.mock.calls.some(([name]) => name === 'plugins_check_update')).toBe(false)
  })
  it('checks and clears a no longer available update through the action menu', async () => {
    const { wrapper } = await mountPage('/plugins?plugin=%40acme%2Freader')
    await click(wrapper, messages.plugins.workflow.more)
    await clickBody(messages.plugins.checkUpdates, 'menuitem')
    expect(mocks.call).toHaveBeenCalledWith('plugins_check_update', { pluginId: '@acme/reader' })
    expect(usePluginsStore().plugin('@acme/reader')?.update).toBeUndefined()
  })
  it('shows a failed list read as an error', async () => {
    answer({ plugins_list: () => ({ ok: false, error: { code: 'unavailable', message: 'error.unavailable' } }) })
    const { wrapper } = await mountPage()
    expect(wrapper.getComponent(ErrorNotice).props('code')).toBe('unavailable')
  })
})
