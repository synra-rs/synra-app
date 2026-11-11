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

function capabilityFields(form: ReturnType<typeof editor> extends Promise<infer Value> ? Value['form'] : never) {
  return form.findAll('[data-permission-capability] input')
}

function checkedCapabilityFields(form: ReturnType<typeof editor> extends Promise<infer Value> ? Value['form'] : never) {
  return capabilityFields(form).filter(field => (field.element as HTMLInputElement).checked)
}

describe('permission decisions', () => {
  it('selects every requested capability with the all checkbox', async () => {
    const { form } = await editor()
    await form.get('[data-testid="permission-all"]').setValue(true)
    expect(checkedCapabilityFields(form)).toHaveLength(2)
    await form.get('[data-testid="permission-all"]').setValue(false)
    expect(checkedCapabilityFields(form)).toHaveLength(0)
  })

  it('opens an awaiting decision when the kernel omits the empty denied list', async () => {
    const plugin = { ...card({ state: 'awaiting_consent', granted: [] }) }
    delete plugin.denied
    answer({ plugins_list: [plugin] })
    const { form } = await editor()
    expect(capabilityFields(form)).toHaveLength(2)
    expect(checkedCapabilityFields(form)).toHaveLength(0)
    expect(form.text()).toContain(messages.plugins.workflow.allowSelected)
  })
  it('accepts an omitted denied list after granting every requested capability', async () => {
    const granted = ['log:*', 'net:fetch:example.com']
    const plugin = { ...card({ granted }) }
    delete plugin.denied
    answer({ plugins_consent_respond: plugin })
    const { form, router, wrapper } = await editor()
    await capabilityFields(form)[1]!.setValue(true)
    await click(form, messages.plugins.consentAction)
    expect(capabilityFields(form)).toHaveLength(0)
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
    const fields = capabilityFields(form)
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
    expect(checkedCapabilityFields(form)).toHaveLength(0)
    await capabilityFields(form)[0]!.setValue(true)
    await click(form, messages.plugins.workflow.allowSelected)
    expect(mocks.call).toHaveBeenCalledWith('plugins_consent_respond', { pluginId: '@acme/reader', granted: ['log:*'] })
  })
  it('cancels without writing', async () => {
    const { form } = await editor()
    await capabilityFields(form)[1]!.setValue(true)
    await click(form, messages.plugins.workflow.cancel)
    expect(mocks.call.mock.calls.some(([command]) => command === 'plugins_consent_respond')).toBe(false)
    expect(capabilityFields(form)).toHaveLength(0)
  })
  it('refreshes a clean draft and preserves dirty edits across events', async () => {
    const { form } = await editor()
    const store = usePluginsStore()
    store.upsert(card({ granted: [], denied: ['log:*', 'net:fetch:example.com'] }))
    await flushPromises()
    expect(checkedCapabilityFields(form)).toHaveLength(0)
    await capabilityFields(form)[1]!.setValue(true)
    const updated = { ...card({ granted: ['log:*', 'net:fetch:example.com'] }) }
    delete updated.denied
    store.apply({ type: 'pluginUpdated', payload: updated })
    await flushPromises()
    expect((capabilityFields(form)[0]!.element as HTMLInputElement).checked).toBe(false)
    expect((capabilityFields(form)[1]!.element as HTMLInputElement).checked).toBe(true)
  })
  it('keeps a failed decision and its draft for retry', async () => {
    answer({ plugins_consent_respond: () => ({ ok: false, error: { code: 'capability_denied', message: 'error.denied' } }) })
    const { form } = await editor()
    await capabilityFields(form)[1]!.setValue(true)
    await click(form, messages.plugins.consentAction)
    expect(form.getComponent(ErrorNotice).props('code')).toBe('capability_denied')
    expect((capabilityFields(form)[1]!.element as HTMLInputElement).checked).toBe(true)
  })
})
