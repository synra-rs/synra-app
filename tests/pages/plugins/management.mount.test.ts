// @vitest-environment jsdom
import { flushPromises } from '@vue/test-utils'
import { Checkbox } from 'ant-design-vue'
import { expect, it } from 'vitest'
import { answer, card, click, clickBody, ErrorNotice, messages, mocks, mountPage, PluginReload, PluginRemoval, usePluginsStore } from './workflow.testkit'

async function removal() {
  const page = await mountPage('/plugins?plugin=%40acme%2Freader')
  await click(page.wrapper, messages.plugins.workflow.more)
  await clickBody(messages.plugins.removeAction, 'menuitem')
  return { ...page, dialog: page.wrapper.getComponent(PluginRemoval) }
}
it('opens routine update details when addedCapabilities is omitted', async () => {
  answer({ plugins_list: [card({ update: { pluginId: '@acme/reader', version: '2.0', needsNewConsent: false } })] })
  await mountPage('/plugins?plugin=%40acme%2Freader')
  expect(document.body.textContent).toContain(messages.plugins.updateNoNewPermissions)
  expect(document.body.textContent).toContain('2.0')
})
it('replaces update checks with a confirmed local reload', async () => {
  answer({ plugins_list: [card({ localSource: true })], plugins_update: { pluginId: '@acme/reader', version: '1.0', enabled: false, warnings: [] } })
  const { wrapper } = await mountPage('/plugins?plugin=%40acme%2Freader')
  await click(wrapper, messages.plugins.workflow.more)
  await clickBody(messages.plugins.workflow.reloadAction, 'menuitem')
  const dialog = wrapper.getComponent(PluginReload)
  await dialog.getComponent(Checkbox).get('input').setValue(true)
  await click(dialog, messages.plugins.workflow.reloadAction)
  expect(mocks.call).toHaveBeenCalledWith('plugins_update', { pluginId: '@acme/reader', removeData: true })
})
it('asks about data and resets the destructive choice after cancellation', async () => {
  const { wrapper, dialog } = await removal()
  expect((dialog.getComponent(Checkbox).get('input').element as HTMLInputElement).checked).toBe(false)
  await dialog.getComponent(Checkbox).get('input').setValue(true)
  await click(dialog, messages.plugins.workflow.cancel)
  await click(wrapper, messages.plugins.workflow.more)
  await clickBody(messages.plugins.removeAction, 'menuitem')
  expect((wrapper.getComponent(PluginRemoval).getComponent(Checkbox).get('input').element as HTMLInputElement).checked).toBe(false)
  expect(mocks.call.mock.calls.some(([command]) => command === 'plugins_uninstall')).toBe(false)
})
it('removes the selected plugin and returns to its list with data kept by default', async () => {
  answer({ plugins_uninstall: { bytesFreed: 0, dataKept: true }, plugins_list: [card()] })
  const { dialog, router } = await removal()
  answer({ plugins_uninstall: { bytesFreed: 0, dataKept: true }, plugins_list: [] })
  await click(dialog, messages.plugins.removeAction)
  expect(mocks.call).toHaveBeenCalledWith('plugins_uninstall', { pluginId: '@acme/reader', removeData: false })
  expect(usePluginsStore().plugin('@acme/reader')).toBeUndefined()
  expect(router.currentRoute.value.query.plugin).toBeUndefined()
})
it('keeps a failed removal in its confirmation', async () => {
  answer({ plugins_uninstall: () => ({ ok: false, error: { code: 'unavailable', message: 'error.unavailable' } }) })
  const { dialog } = await removal()
  await click(dialog, messages.plugins.removeAction)
  expect(dialog.getComponent(ErrorNotice).props('code')).toBe('unavailable')
  expect(usePluginsStore().plugin('@acme/reader')).toBeDefined()
})
it('keeps the report from an update and its new permission request', async () => {
  answer({ plugins_list: [card({ update: { pluginId: '@acme/reader', version: '2.0', needsNewConsent: true, addedCapabilities: ['fs:read'] } })], plugins_update: { pluginId: '@acme/reader', version: '2.0', enabled: false, warnings: ['Review the changed permissions'] } })
  const { wrapper } = await mountPage('/plugins?plugin=%40acme%2Freader')
  await click(wrapper, messages.plugins.workflow.more)
  await clickBody(messages.plugins.updateAction, 'menuitem')
  expect(wrapper.text()).toContain('2.0')
  expect(wrapper.text()).toContain('Review the changed permissions')
})
it('binds publisher trust to the full fingerprint and preserves failed decisions', async () => {
  const publisher = { publisherId: '@acme', fingerprint: 'full-fingerprint-from-the-kernel', trust: 'seen' }
  answer({ plugins_publishers: [publisher], plugins_publisher_decide: () => ({ ok: false, error: { code: 'invalid_argument', message: 'error.stale' } }) })
  const { wrapper } = await mountPage()
  await click(wrapper, messages.plugins.publishers.manage(1))
  await click(wrapper, messages.plugins.publishers.trust)
  expect(mocks.call).toHaveBeenCalledWith('plugins_publisher_decide', { publisherId: publisher.publisherId, fingerprint: publisher.fingerprint, trust: 'trusted' })
  await flushPromises()
  expect(document.body.textContent).toContain(publisher.fingerprint)
  expect(wrapper.findAllComponents(ErrorNotice).some(notice => notice.props('code') === 'invalid_argument')).toBe(true)
})
