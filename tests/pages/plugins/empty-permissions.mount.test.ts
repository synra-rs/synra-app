// @vitest-environment jsdom
import { expect, it } from 'vitest'
import { answer, card, click, messages, mountPage, PluginInstallConsent } from './workflow.testkit'

it('confirms a zero-permission plugin without inventing grants', async () => {
  answer({ plugins_list: [card({ state: 'awaiting_consent', granted: [], denied: [], requested: [] })], plugins_consent_respond: card({ state: 'active', granted: [], denied: [], requested: [] }) })
  const { wrapper } = await mountPage('/plugins?plugin=%40acme%2Freader')
  const consent = wrapper.getComponent(PluginInstallConsent)
  expect(document.body.textContent).toContain(messages.plugins.preview.none)
  await click(consent, messages.plugins.workflow.acceptInstall)
  expect(consent.emitted('close')).toBeTruthy()
})

it('does not show a confirmation for an active zero-permission plugin', async () => {
  answer({ plugins_list: [card({ state: 'active', requested: [], granted: [], denied: [] })] })
  const { wrapper } = await mountPage('/plugins?plugin=%40acme%2Freader')
  expect(wrapper.findComponent(PluginInstallConsent).exists()).toBe(false)
})
