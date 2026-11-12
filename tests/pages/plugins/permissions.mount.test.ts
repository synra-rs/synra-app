// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { answer, card, clickBody, messages, mountPage, PluginPermissions } from './workflow.testkit'

describe('permission display', () => {
  it('shows every requested permission without selection controls', async () => {
    const { wrapper } = await mountPage('/plugins?plugin=%40acme%2Freader')
    await clickBody(messages.plugins.workflow.permissions, 'tab')
    const permissions = wrapper.getComponent(PluginPermissions)
    expect(permissions.findAll('input,button')).toHaveLength(0)
    expect(permissions.text()).toContain('log:*')
    expect(permissions.text()).toContain('net:fetch:example.com')
    expect(permissions.text()).toContain(messages.plugins.workflow.allowed)
    expect(permissions.text()).toContain(messages.plugins.workflow.denied)
  })

  it('shows no permissions when the plugin requests none', async () => {
    answer({ plugins_list: [card({ requested: [], granted: [], denied: [] })] })
    const { wrapper } = await mountPage('/plugins?plugin=%40acme%2Freader')
    await clickBody(messages.plugins.workflow.permissions, 'tab')
    const permissions = wrapper.getComponent(PluginPermissions)
    expect(permissions.text()).toContain(messages.plugins.preview.none)
    expect(permissions.findAll('input,button')).toHaveLength(0)
  })
})
