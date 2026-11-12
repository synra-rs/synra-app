// @vitest-environment jsdom
import { flushPromises } from '@vue/test-utils'
import { Input } from 'ant-design-vue'
import { describe, expect, it } from 'vitest'
import { answer, card, click, ErrorNotice, LocalInstallDialog, messages, mocks, mountPage, PluginInstallConsent } from './workflow.testkit'

describe('local installation', () => {
  it('opens one all-or-nothing permission confirmation after installation', async () => {
    answer({
      plugins_list: [card({ enabled: false, state: 'disabled', granted: [], denied: [] })],
      plugins_install: { pluginId: '@acme/reader', version: '1.0', enabled: false },
      plugins_consent_respond: card({ enabled: false, state: 'disabled', granted: ['log:*', 'net:fetch:example.com'], denied: [] })
    })
    mocks.open.mockResolvedValue('D:/reader.synra')
    const { wrapper } = await mountPage()
    await click(wrapper, messages.plugins.workflow.localInstall)
    const dialog = wrapper.getComponent(LocalInstallDialog)
    await click(dialog, messages.plugins.choosePackage)
    await click(dialog, messages.plugins.installAction)
    const consent = wrapper.getComponent(PluginInstallConsent)
    expect(document.body.textContent).toContain(messages.plugins.workflow.installConsentTitle)
    expect(document.body.textContent).toContain('log:*')
    await click(consent, messages.plugins.workflow.acceptInstall)
    expect(mocks.call).toHaveBeenCalledWith('plugins_consent_respond', { pluginId: '@acme/reader', granted: ['log:*', 'net:fetch:example.com'] })
  })

  it('removes the newly installed plugin when permission is rejected', async () => {
    answer({
      plugins_list: [card({ enabled: false, state: 'disabled', granted: [], denied: [] })],
      plugins_install: { pluginId: '@acme/reader', version: '1.0', enabled: false },
      plugins_uninstall: { bytesFreed: 0, dataKept: true, leftovers: [] }
    })
    mocks.open.mockResolvedValue('D:/reader.synra')
    const { wrapper } = await mountPage()
    await click(wrapper, messages.plugins.workflow.localInstall)
    const dialog = wrapper.getComponent(LocalInstallDialog)
    await click(dialog, messages.plugins.choosePackage)
    await click(dialog, messages.plugins.installAction)
    await click(wrapper.getComponent(PluginInstallConsent), messages.plugins.workflow.rejectInstall)
    expect(mocks.call).toHaveBeenCalledWith('plugins_uninstall', { pluginId: '@acme/reader', removeData: false })
  })

  it('does not start reads or installations until the dialog is used', async () => {
    const { wrapper } = await mountPage()
    expect(wrapper.findComponent(LocalInstallDialog).exists()).toBe(false)
    await click(wrapper, messages.plugins.workflow.localInstall)
    expect(wrapper.findComponent(LocalInstallDialog).exists()).toBe(true)
    expect(mocks.call.mock.calls.some(([command]) => command === 'plugins_preview' || command === 'plugins_install')).toBe(false)
  })

  it.each([true, false])('chooses a native source and installs its path with directory=%s', async (directory) => {
    answer({ plugins_install: { pluginId: '@acme/reader', version: '1.0', enabled: false } })
    mocks.open.mockResolvedValue('D:/plugins/reader.synra')
    const { wrapper } = await mountPage()
    await click(wrapper, messages.plugins.workflow.localInstall)
    const dialog = wrapper.getComponent(LocalInstallDialog)
    await click(dialog, directory ? messages.plugins.chooseDirectory : messages.plugins.choosePackage)
    expect(mocks.open).toHaveBeenCalledWith(expect.objectContaining({ directory, multiple: false }))
    await click(dialog, messages.plugins.installAction)
    expect(mocks.call).toHaveBeenCalledWith('plugins_install', { source: { source: 'path', path: 'D:/plugins/reader.synra' } })
    expect(wrapper.findComponent(LocalInstallDialog).exists()).toBe(false)
    expect(wrapper.findComponent(PluginInstallConsent).exists()).toBe(true)
  })

  it('retains a rejected path and a picker cancellation leaves it intact', async () => {
    answer({ plugins_install: () => ({ ok: false, error: { code: 'invalid_argument', message: 'error.invalid' } }) })
    mocks.open.mockResolvedValueOnce('D:/chosen').mockResolvedValueOnce(null)
    const { wrapper } = await mountPage()
    await click(wrapper, messages.plugins.workflow.localInstall)
    const dialog = wrapper.getComponent(LocalInstallDialog)
    await click(dialog, messages.plugins.chooseDirectory)
    await click(dialog, messages.plugins.installAction)
    expect(dialog.getComponent(ErrorNotice).props('code')).toBe('invalid_argument')
    await click(dialog, messages.plugins.choosePackage)
    expect(dialog.getComponent(Input).get('input').element.value).toBe('D:/chosen')
  })

  it('does not submit a composing Enter', async () => {
    const { wrapper } = await mountPage()
    await click(wrapper, messages.plugins.workflow.localInstall)
    const dialog = wrapper.getComponent(LocalInstallDialog)
    await dialog.getComponent(Input).get('input').setValue('D:/manual')
    await dialog.getComponent(Input).get('input').trigger('keydown', { key: 'Enter', isComposing: true })
    await flushPromises()
    expect(mocks.call.mock.calls.some(([command]) => command === 'plugins_install')).toBe(false)
  })

  it('keeps a composing Escape inside the input method', async () => {
    const { wrapper } = await mountPage()
    await click(wrapper, messages.plugins.workflow.localInstall)
    const dialog = wrapper.getComponent(LocalInstallDialog)
    await dialog.getComponent(Input).get('input').trigger('keydown', { key: 'Escape', keyCode: 27, isComposing: true })
    await flushPromises()
    expect(wrapper.findComponent(LocalInstallDialog).exists()).toBe(true)
  })
})
