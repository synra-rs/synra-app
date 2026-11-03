// @vitest-environment jsdom
import { flushPromises } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { answer, card, click, clickBody, ErrorNotice, messages, mocks, mountPage, PluginPermissions, usePluginsStore } from './workflow.testkit'

async function editor() {
  const page = await mountPage('/plugins?plugin=%40acme%2Freader')
  await clickBody(messages.plugins.workflow.permissions, 'tab')
  const form = page.wrapper.getComponent(PluginPermissions)
  if (form.findAll('input[type="checkbox"]').length === 0)
    await click(form, messages.plugins.workflow.edit)
  return { ...page, form }
}

describe('permission decisions', () => {
  it('opens an awaiting decision when the kernel omits the empty denied list', async () => {
    const plugin = { ...card({ state: 'awaiting_consent', granted: [] }) }
    delete plugin.denied
    answer({ plugins_list: [plugin] })
    const { form } = await editor()
    expect(form.findAll('input[type="checkbox"]')).toHaveLength(2)
    expect(form.findAll('input:checked')).toHaveLength(0)
    expect(form.text()).toContain(messages.plugins.workflow.allowSelected)
  })
  it('accepts an omitted denied list after granting every requested capability', async () => {
    const granted = ['log:*', 'net:fetch:example.com']
    const plugin = { ...card({ granted }) }
    delete plugin.denied
    answer({ plugins_consent_respond: plugin })
    const { form, router, wrapper } = await editor()
    await form.findAll('input[type="checkbox"]')[1]!.setValue(true)
    await click(form, messages.plugins.consentAction)
    expect(form.findAll('input[type="checkbox"]')).toHaveLength(0)
    expect(form.text()).not.toContain(messages.plugins.workflow.newPermission)
    await router.push('/plugins')
    await flushPromises()
    expect(wrapper.findComponent(PluginPermissions).exists()).toBe(false)
    await router.push('/plugins?plugin=%40acme%2Freader&tab=permissions')
    await flushPromises()
    await clickBody(messages.plugins.workflow.permissions, 'tab')
    expect(wrapper.findComponent(PluginPermissions).exists()).toBe(true)
  })
  it('shows no permissions when both empty optional lists are omitted', async () => {
    const plugin = { ...card({ granted: [] }) }
    delete plugin.denied
    delete plugin.requested
    answer({ plugins_list: [plugin] })
    const page = await mountPage('/plugins?plugin=%40acme%2Freader&tab=permissions')
    await clickBody(messages.plugins.workflow.permissions, 'tab')
    const form = page.wrapper.getComponent(PluginPermissions)
    expect(form.text()).toContain(messages.plugins.preview.none)
    expect(form.findAll('input[type="checkbox"]')).toHaveLength(0)
  })
  it('saves the selected subset and leaves other requests denied', async () => {
    answer({ plugins_consent_respond: ({ granted }: { granted: string[] }) => ({ ok: true, value: card({ granted, denied: ['net:fetch:example.com'] }) }) })
    const { form } = await editor()
    const fields = form.findAll('input[type="checkbox"]')
    expect((fields[0]!.element as HTMLInputElement).checked).toBe(true)
    expect((fields[1]!.element as HTMLInputElement).checked).toBe(false)
    await fields[0]!.setValue(false)
    await fields[0]!.setValue(true)
    await fields[1]!.setValue(true)
    await click(form, messages.plugins.consentAction)
    expect(mocks.call).toHaveBeenCalledWith('plugins_consent_respond', { pluginId: '@acme/reader', granted: ['log:*', 'net:fetch:example.com'] })
  })
  it('supports initial partial consent without automatically selecting new requests', async () => {
    answer({ plugins_list: [card({ state: 'awaiting_consent', granted: [], denied: [] })], plugins_consent_respond: ({ granted }: { granted: string[] }) => ({ ok: true, value: card({ granted }) }) })
    const { form } = await editor()
    expect(form.findAll('input:checked')).toHaveLength(0)
    await form.findAll('input[type="checkbox"]')[0]!.setValue(true)
    await click(form, messages.plugins.workflow.allowSelected)
    expect(mocks.call).toHaveBeenCalledWith('plugins_consent_respond', { pluginId: '@acme/reader', granted: ['log:*'] })
  })
  it('cancels without writing', async () => {
    const { form } = await editor()
    await form.findAll('input[type="checkbox"]')[1]!.setValue(true)
    await click(form, messages.plugins.workflow.cancel)
    expect(mocks.call.mock.calls.some(([command]) => command === 'plugins_consent_respond')).toBe(false)
    expect(form.findAll('input[type="checkbox"]')).toHaveLength(0)
  })
  it('refreshes a clean draft and preserves dirty edits across events', async () => {
    const { form } = await editor()
    const store = usePluginsStore()
    store.upsert(card({ granted: [], denied: ['log:*', 'net:fetch:example.com'] }))
    await flushPromises()
    expect(form.findAll('input:checked')).toHaveLength(0)
    await form.findAll('input[type="checkbox"]')[1]!.setValue(true)
    const updated = { ...card({ granted: ['log:*', 'net:fetch:example.com'] }) }
    delete updated.denied
    store.apply({ type: 'pluginUpdated', payload: updated })
    await flushPromises()
    expect((form.findAll('input[type="checkbox"]')[0]!.element as HTMLInputElement).checked).toBe(false)
    expect((form.findAll('input[type="checkbox"]')[1]!.element as HTMLInputElement).checked).toBe(true)
  })
  it('keeps a failed decision and its draft for retry', async () => {
    answer({ plugins_consent_respond: () => ({ ok: false, error: { code: 'capability_denied', message: 'error.denied' } }) })
    const { form } = await editor()
    await form.findAll('input[type="checkbox"]')[1]!.setValue(true)
    await click(form, messages.plugins.consentAction)
    expect(form.getComponent(ErrorNotice).props('code')).toBe('capability_denied')
    expect((form.findAll('input[type="checkbox"]')[1]!.element as HTMLInputElement).checked).toBe(true)
  })
})
