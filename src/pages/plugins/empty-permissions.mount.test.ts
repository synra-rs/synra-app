// @vitest-environment jsdom
import { expect, it } from 'vitest'
import { answer, card, click, clickBody, ErrorNotice, LocalInstallDialog, messages, mocks, mountPage, PluginPermissions, usePluginsStore } from './workflow.testkit'

function emptyPlugin(state = 'awaiting_consent', enabled = true) {
  const plugin = { ...card({ state, enabled, granted: [] }) }
  delete plugin.requested
  delete plugin.denied
  return plugin
}

it('approves running a zero-permission plugin without inventing any grants', async () => {
  answer({ plugins_list: [emptyPlugin()], plugins_consent_respond: emptyPlugin('active') })
  const { wrapper } = await mountPage('/plugins?plugin=%40acme%2Freader')
  await clickBody(messages.plugins.workflow.permissions, 'tab')
  const form = wrapper.getComponent(PluginPermissions)
  expect(form.text()).toContain(messages.plugins.preview.none)
  expect(form.findAll('input[type="checkbox"]')).toHaveLength(0)
  await click(form, messages.plugins.workflow.allowRun)
  expect(mocks.call).toHaveBeenCalledWith('plugins_consent_respond', { pluginId: '@acme/reader', granted: [] })
  expect(usePluginsStore().plugin('@acme/reader')?.state).toBe('active')
  expect(form.text()).not.toContain(messages.plugins.workflow.allowRun)
})

it('can approve a disabled newly installed plugin while leaving it disabled', async () => {
  answer({ plugins_list: [emptyPlugin('disabled', false)], plugins_install: { pluginId: '@acme/reader', version: '1.0', enabled: false }, plugins_consent_respond: emptyPlugin('disabled', false) })
  mocks.open.mockResolvedValue('D:/reader.synra')
  const { wrapper } = await mountPage()
  await click(wrapper, messages.plugins.workflow.localInstall)
  const dialog = wrapper.getComponent(LocalInstallDialog)
  await click(dialog, messages.plugins.choosePackage)
  await click(dialog, messages.plugins.installAction)
  await click(wrapper, messages.plugins.reviewPermissions)
  const form = wrapper.getComponent(PluginPermissions)
  await click(form, messages.plugins.workflow.allowRun)
  expect(mocks.call).toHaveBeenCalledWith('plugins_consent_respond', { pluginId: '@acme/reader', granted: [] })
  expect(usePluginsStore().plugin('@acme/reader')?.enabled).toBe(false)
  expect(form.text()).not.toContain(messages.plugins.workflow.allowRun)
})

it('keeps a failed approval available for retry', async () => {
  answer({ plugins_list: [emptyPlugin()], plugins_consent_respond: () => ({ ok: false, error: { code: 'internal', message: 'error.internal' } }) })
  const { wrapper } = await mountPage('/plugins?plugin=%40acme%2Freader')
  await clickBody(messages.plugins.workflow.permissions, 'tab')
  const form = wrapper.getComponent(PluginPermissions)
  await click(form, messages.plugins.workflow.allowRun)
  expect(form.getComponent(ErrorNotice).props('code')).toBe('internal')
  expect(form.text()).toContain(messages.plugins.workflow.allowRun)
  expect(usePluginsStore().plugin('@acme/reader')?.state).toBe('awaiting_consent')
})

it('shows no approval action for an active zero-permission plugin', async () => {
  answer({ plugins_list: [emptyPlugin('active')] })
  const { wrapper } = await mountPage('/plugins?plugin=%40acme%2Freader')
  await clickBody(messages.plugins.workflow.permissions, 'tab')
  const form = wrapper.getComponent(PluginPermissions)
  expect(form.text()).toContain(messages.plugins.preview.none)
  expect(form.findAll('button')).toHaveLength(0)
})
