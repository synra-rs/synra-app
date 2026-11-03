// @vitest-environment jsdom
import { expect, it } from 'vitest'
import { answer, card, clickBody, messages, mountPage, PluginPermissions } from './workflow.testkit'

it('names the permission region and associates every requested scope with its checkbox', async () => {
  answer({ plugins_list: [card({ state: 'awaiting_consent', granted: [] })] })
  const { wrapper } = await mountPage('/plugins?plugin=%40acme%2Freader')
  await clickBody(messages.plugins.workflow.permissions, 'tab')
  const form = wrapper.getComponent(PluginPermissions)
  expect(form.get('section').attributes('aria-label')).toBe(messages.plugins.workflow.permissions)
  const labels = form.findAll('label')
  expect(labels).toHaveLength(2)
  expect(labels[0]!.text()).toContain('log:*')
  expect(labels[1]!.text()).toContain('net:fetch:example.com')
  expect(labels.every(label => label.find('input[type="checkbox"]').exists())).toBe(true)
})
