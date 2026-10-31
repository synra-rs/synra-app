// @vitest-environment jsdom
import type { CredentialSnapshot } from '@synra/protocol'
import { flushPromises, mount } from '@vue/test-utils'
import { Modal } from 'ant-design-vue'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, expect, it, vi } from 'vitest'

import { IDENTITY } from '../ipc/fake-kernel-fixtures'
import { messages } from '../messages'
import { useCredentialsStore } from '../stores/credentials'
import CredentialPanel from './CredentialPanel.vue'
import ErrorNotice from './ErrorNotice.vue'

const { call } = vi.hoisted(() => ({ call: vi.fn() }))
vi.mock('../kernel', () => ({ kernel: { call } }))

let backend: CredentialSnapshot
beforeEach(() => {
  window.matchMedia = vi.fn().mockReturnValue({
    matches: false,
    addListener() {},
    removeListener() {},
    addEventListener() {},
    removeEventListener() {}
  })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  setActivePinia(createPinia())
  backend = { deviceId: IDENTITY.deviceId, enrollment: { fingerprint: 'displayed', backend: 'windows', ready: true }, recovery: null, retiredKeys: 0, runtimeReady: true, restartRequired: false }
  call.mockReset()
  call.mockImplementation(async (command: string, args?: { expectedFingerprint?: string }) => {
    if (command === 'identity_credentials')
      return { ok: true, value: backend }
    if (command === 'identity_repair') {
      if (args?.expectedFingerprint !== (backend.enrollment ?? backend.recovery)?.fingerprint)
        return { ok: false, error: { code: 'invalid_argument', message: 'error.invalid_argument' } }
      backend = { ...backend, enrollment: { fingerprint: 'replacement', backend: 'windows', ready: true }, recovery: null, runtimeReady: false, restartRequired: true, retiredKeys: 1 }
      return { ok: true, value: { deviceId: IDENTITY.deviceId, fingerprint: 'replacement', restartRequired: true, cleanup: { remaining: null, errorCode: 'internal', detail: 'cleanup failed' } } }
    }
    if (command === 'identity_cleanup') {
      backend = { ...backend, retiredKeys: 0 }
      return { ok: true, value: { remaining: 0, errorCode: null, detail: null } }
    }
    if (command === 'devices_snapshot')
      return { ok: true, value: { identity: IDENTITY, devices: [] } }
    throw new Error(command)
  })
})

it('requires confirmation of the displayed fingerprint and keeps it fixed when a refresh changes enrollment', async () => {
  const wrapper = mount(CredentialPanel)
  await flushPromises()
  await wrapper.findAll('button').find(node => node.text() === messages.settings.credentials.repair)!.trigger('click')
  expect(wrapper.findComponent(Modal).props('open')).toBe(true)
  expect(call.mock.calls.some(([command]) => command === 'identity_repair')).toBe(false)
  backend = { ...backend, enrollment: { fingerprint: 'changed', backend: 'windows', ready: true } }
  await useCredentialsStore().refresh()
  wrapper.findComponent(Modal).vm.$emit('ok')
  await flushPromises()
  expect(call).toHaveBeenCalledWith('identity_repair', { expectedFingerprint: 'displayed' })
  expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('invalid_argument')
  expect(useCredentialsStore().repaired).toBeNull()
  await wrapper.findAll('button').find(node => node.text() === messages.settings.credentials.repair)!.trigger('click')
  wrapper.findComponent(Modal).vm.$emit('ok')
  await flushPromises()
  expect(call).toHaveBeenCalledWith('identity_repair', { expectedFingerprint: 'changed' })
  expect(wrapper.text()).toContain(messages.settings.credentials.repaired)
  expect(wrapper.text()).toContain(messages.settings.credentials.restart)
  expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('internal')
  await wrapper.findAll('button').find(node => node.text() === messages.settings.credentials.cleanup)!.trigger('click')
  await flushPromises()
  expect(wrapper.findComponent(ErrorNotice).exists()).toBe(false)
  expect(wrapper.text()).toContain(messages.settings.credentials.repaired)
  wrapper.unmount()
})

it('allows repair when startup could not load the key and permits cancelling without a write', async () => {
  backend = { ...backend, runtimeReady: false, restartRequired: true }
  const wrapper = mount(CredentialPanel)
  await flushPromises()
  const repair = wrapper.findAll('button').find(node => node.text() === messages.settings.credentials.repair)!
  expect(repair.attributes('disabled')).toBeUndefined()
  await repair.trigger('click')
  wrapper.findComponent(Modal).vm.$emit('cancel')
  await flushPromises()
  expect(wrapper.findComponent(Modal).props('open')).toBe(false)
  expect(call.mock.calls.some(([command]) => command === 'identity_repair')).toBe(false)
  wrapper.unmount()
})
it('requires explicit confirmation to recover a missing registration using the retained fingerprint', async () => {
  backend = { ...backend, enrollment: null, recovery: { fingerprint: 'retained', backend: 'windows' }, runtimeReady: false }
  const wrapper = mount(CredentialPanel)
  await flushPromises()
  expect(wrapper.text()).toContain('retained')
  expect(wrapper.text()).toContain(messages.settings.credentials.registrationLost)
  expect(call.mock.calls.some(([command]) => command === 'identity_repair')).toBe(false)
  await wrapper.findAll('button').find(node => node.text() === messages.settings.credentials.repair)!.trigger('click')
  wrapper.findComponent(Modal).vm.$emit('ok')
  await flushPromises()
  expect(call).toHaveBeenCalledWith('identity_repair', { expectedFingerprint: 'retained' })
  expect(useCredentialsStore().snapshot?.recovery).toBeNull()
  expect(wrapper.text()).toContain(messages.settings.credentials.repaired)
  wrapper.unmount()
})

it('explains the trusted backup requirement when no registration or recovery record survives', async () => {
  call.mockResolvedValue({ ok: false, error: { code: 'not_paired', message: 'error.identityHistoryMissing' } })
  const wrapper = mount(CredentialPanel)
  await flushPromises()
  expect(wrapper.text()).toContain(messages.errors.not_paired)
  expect(wrapper.findAll('button').some(node => node.text() === messages.settings.credentials.repair)).toBe(false)
  expect(call.mock.calls.some(([command]) => command === 'identity_repair')).toBe(false)
  wrapper.unmount()
})

it('recovers from a read failure through the refresh button', async () => {
  // The notice carries `@retry`, but a read refusal has no actions, so the way back is the separate refresh button
  // that is always on the panel. This pins that route rather than a button the kernel may or may not ask for.
  let failing = true
  call.mockImplementation(async (command: string) => {
    if (command === 'identity_credentials') {
      return failing
        ? { ok: false, error: { code: 'internal', message: 'error.internal' } }
        : { ok: true, value: backend }
    }
    if (command === 'devices_snapshot')
      return { ok: true, value: { identity: IDENTITY, devices: [] } }
    throw new Error(command)
  })
  const wrapper = mount(CredentialPanel)
  await flushPromises()
  expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('internal')
  failing = false
  await wrapper.findAll('button').find(node => node.text() === messages.settings.credentials.refresh)!.trigger('click')
  await flushPromises()
  expect(wrapper.findComponent(ErrorNotice).exists()).toBe(false)
  expect(wrapper.text()).toContain(backend.enrollment!.fingerprint)
  wrapper.unmount()
})

it('keeps a failed cleanup visible instead of leaving only the repair note', async () => {
  // The cleanup is the destructive half of a repair: it revokes the retired keys. A refusal there must not leave the
  // screen saying only "repaired" while the keys are still usable.
  backend = { ...backend, retiredKeys: 1, restartRequired: true, runtimeReady: false }
  const base = call.getMockImplementation()!
  call.mockImplementation(async (command: string, args?: { expectedFingerprint?: string }) => {
    if (command === 'identity_cleanup')
      return { ok: false, error: { code: 'internal', message: 'error.internal' } }
    if (command === 'identity_repair')
      return { ok: true, value: { deviceId: IDENTITY.deviceId, fingerprint: 'replacement', restartRequired: true, cleanup: { remaining: 1, errorCode: null, detail: null } } }
    return base(command, args)
  })
  const wrapper = mount(CredentialPanel)
  await flushPromises()
  await wrapper.findAll('button').find(node => node.text() === messages.settings.credentials.repair)!.trigger('click')
  wrapper.findComponent(Modal).vm.$emit('ok')
  await flushPromises()
  await wrapper.findAll('button').find(node => node.text() === messages.settings.credentials.cleanup)!.trigger('click')
  await flushPromises()
  expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('internal')
  // And the warning about what is still there has to survive alongside it, or the two halves disagree.
  expect(wrapper.text()).toContain(messages.settings.credentials.remaining(1))
  wrapper.unmount()
})
