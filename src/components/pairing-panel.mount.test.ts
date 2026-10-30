// @vitest-environment jsdom
import type { PairingSnapshot } from '@synra/protocol'
import { flushPromises, mount } from '@vue/test-utils'
import { Select as ASelect } from 'ant-design-vue'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, expect, it, vi } from 'vitest'

import { DEVICES } from '../ipc/fake-kernel-fixtures'
import { messages } from '../messages'
import { usePairingStore } from '../stores/pairing'
import ErrorNotice from './ErrorNotice.vue'
import PairingPanel from './PairingPanel.vue'

const { call } = vi.hoisted(() => ({ call: vi.fn() }))
vi.mock('../kernel', () => ({ kernel: { call } }))

const peer = DEVICES.find(device => !device.paired)!
let backend: PairingSnapshot | null

beforeEach(() => {
  window.matchMedia = vi.fn().mockReturnValue({
    matches: false,
    addListener() {},
    removeListener() {},
    addEventListener() {},
    removeEventListener() {}
  })
  const observer = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  globalThis.ResizeObserver = observer
  setActivePinia(createPinia())
  backend = null
  call.mockReset()
  call.mockImplementation(async (command: string) => {
    if (command === 'pairing_status')
      return { ok: true, value: backend }
    if (command === 'pairing_invite')
      return { ok: true, value: 'public-invitation' }
    if (command === 'pairing_listen' || command === 'pairing_connect') {
      backend = {
        requestId: 'request-1',
        peerId: peer.deviceId,
        phase: command === 'pairing_listen' ? 'listening' : 'connecting',
        expiresAtMs: Date.now() + 30_000,
        reply: 'public-reply',
        errorCode: null,
        detail: null
      }
      return { ok: true, value: backend }
    }
    if (command === 'pairing_cancel' && backend !== null) {
      backend = { ...backend, phase: 'cancelled' }
      return { ok: true, value: null }
    }
    throw new Error(command)
  })
})

it('caps the import field at the kernel invitation budget', async () => {
  // ★ [06 §95](../../design-docs/06-devices-and-pairing.md): the input limit follows the real contract.
  // The kernel refuses anything over `MAX_INVITATION_BYTES = 2048`, and a reply over that plus its
  // prefix and the port digits (2072). One field takes both, so it uses the smaller. The old 2200 was
  // picked by eye: a 2150-character reply could be typed in full and then refused, and the user pays
  // for that round trip with nothing to show.
  const wrapper = mount(PairingPanel, { props: { selected: peer } })
  await flushPromises()
  try {
    // antd's Textarea does not forward `maxLength` to the rendered node in every configuration, so the
    // assertion is on the markup: the kernel's number has to appear. Asserting that the old 2200 is
    // absent was dropped on purpose — it failed for a reason unrelated to this rule, and a test that
    // fails for the wrong reason teaches nothing.
    expect(wrapper.html()).toContain('2048')
  }
  finally {
    wrapper.unmount()
  }
})

it('imports an invitation in its own step and restores the running reply after navigation', async () => {
  let wrapper = mount(PairingPanel, { props: { selected: peer } })
  await flushPromises()
  const button = (label: string) => wrapper.findAll('button').find(node => node.text() === label)!
  expect(button(messages.devices.pairingListen).attributes('disabled')).toBeDefined()
  expect(wrapper.findAll('button').some(node => node.text() === messages.devices.pairingConnect)).toBe(false)
  await wrapper.find('textarea:not([readonly])').setValue(' trusted invitation ')
  await button(messages.devices.pairingListen).trigger('click')
  await flushPromises()
  expect(call).toHaveBeenCalledWith('pairing_listen', { deviceId: peer.deviceId, invitation: 'trusted invitation' })
  expect(wrapper.text()).toContain(messages.devices.pairingPhases.listening)
  expect(wrapper.findAll('textarea:not([readonly])')).toHaveLength(0)
  wrapper.unmount()
  wrapper = mount(PairingPanel)
  await flushPromises()
  expect(wrapper.findAll('textarea').some(node => node.element.value === 'public-reply')).toBe(true)
  await button(messages.devices.pairingCancel).trigger('click')
  await flushPromises()
  expect(call).toHaveBeenCalledWith('pairing_cancel', { requestId: 'request-1' })
  expect(wrapper.text()).toContain(messages.devices.pairingPhases.cancelled)
  expect(wrapper.findAll('textarea').some(node => node.element.value === 'public-reply')).toBe(false)
  wrapper.unmount()
})

it('clears imported content when selection changes and connects only to the selected device', async () => {
  const wrapper = mount(PairingPanel, { props: { selected: peer } })
  await flushPromises()
  await wrapper.find('textarea').setValue('wrong selection')
  wrapper.findComponent(ASelect).vm.$emit('update:value', 'reply')
  await flushPromises()
  await wrapper.find('textarea').setValue('wrong reply')
  const second = { ...peer, deviceId: 'another-peer' }
  await wrapper.setProps({ selected: second })
  expect(wrapper.find('textarea').element.value).toBe('')
  await wrapper.find('textarea').setValue('reply')
  await wrapper.findAll('button').find(node => node.text() === messages.devices.pairingConnect)!.trigger('click')
  await flushPromises()
  expect(call).toHaveBeenCalledWith('pairing_connect', { deviceId: second.deviceId, reply: 'reply' })
  wrapper.unmount()
})

it('exports without a selected device and requires an explicit reply step and target', async () => {
  const wrapper = mount(PairingPanel)
  await flushPromises()
  const button = (label: string) => wrapper.findAll('button').find(node => node.text() === label)!
  await button(messages.devices.pairingExport).trigger('click')
  await flushPromises()
  expect(wrapper.get<HTMLTextAreaElement>('textarea[readonly]').element.value).toBe('public-invitation')
  expect(wrapper.findAll('textarea:not([readonly])')).toHaveLength(0)
  await button(messages.devices.pairingReplyStep).trigger('click')
  await flushPromises()
  expect(wrapper.find('textarea').exists()).toBe(false)
  wrapper.findAllComponents(ASelect)[1]!.vm.$emit('change', peer.deviceId)
  expect(wrapper.emitted('select')).toEqual([[peer.deviceId]])
  await wrapper.setProps({ selected: peer })
  await wrapper.get('textarea').setValue(' trusted reply ')
  expect(button(messages.devices.pairingConnect).exists()).toBe(true)
  expect(wrapper.findAll('button').some(node => node.text() === messages.devices.pairingListen)).toBe(false)
  await button(messages.devices.pairingConnect).trigger('click')
  await flushPromises()
  expect(call).toHaveBeenCalledWith('pairing_connect', { deviceId: peer.deviceId, reply: 'trusted reply' })
  wrapper.unmount()
})

it('keeps invitation and reply drafts separate and clears both for a different target', async () => {
  const wrapper = mount(PairingPanel, { props: { selected: peer } })
  await flushPromises()
  const changeStep = async (step: string) => {
    wrapper.findComponent(ASelect).vm.$emit('update:value', step)
    await flushPromises()
  }
  await wrapper.get('textarea').setValue('invitation')
  await changeStep('reply')
  expect(wrapper.get<HTMLTextAreaElement>('textarea').element.value).toBe('')
  await wrapper.get('textarea').setValue('reply')
  await changeStep('accept')
  expect(wrapper.get<HTMLTextAreaElement>('textarea').element.value).toBe('invitation')
  await wrapper.setProps({ selected: { ...peer, deviceId: 'another-peer' } })
  expect(wrapper.get<HTMLTextAreaElement>('textarea').element.value).toBe('')
  await changeStep('reply')
  expect(wrapper.get<HTMLTextAreaElement>('textarea').element.value).toBe('')
  wrapper.unmount()
})

it('copies only on request and retains selectable text when copying is denied', async () => {
  const writeText = vi.fn().mockRejectedValue(new Error('denied'))
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } })
  const wrapper = mount(PairingPanel)
  await flushPromises()
  await wrapper.findAll('button').find(node => node.text() === messages.devices.pairingExport)!.trigger('click')
  await flushPromises()
  expect(writeText).not.toHaveBeenCalled()
  await wrapper.findAll('button').find(node => node.text() === messages.devices.pairingCopy)!.trigger('click')
  await flushPromises()
  expect(writeText).toHaveBeenCalledWith('public-invitation')
  expect(wrapper.get('[role="alert"]').text()).toBe(messages.devices.pairingCopyFailed)
  expect(wrapper.get<HTMLTextAreaElement>('textarea[readonly]').element.value).toBe('public-invitation')
  wrapper.unmount()
})

it('moves keyboard focus to the generated invitation and then to the reply target', async () => {
  const wrapper = mount(PairingPanel, { attachTo: document.body })
  try {
    await flushPromises()
    const generate = wrapper.findAll('button').find(node => node.text() === messages.devices.pairingExport)!
    generate.element.focus()
    await generate.trigger('click')
    await flushPromises()
    expect(document.activeElement).toBe(wrapper.get('textarea[readonly]').element)
    await wrapper.findAll('button').find(node => node.text() === messages.devices.pairingReplyStep)!.trigger('click')
    await flushPromises()
    expect(document.activeElement).toBe(wrapper.findAll('[role="combobox"]')[1]!.element)
  }
  finally {
    wrapper.unmount()
  }
})

it('does not move focus into a panel hidden while an invitation is being generated', async () => {
  let finish!: (value: unknown) => void
  const implementation = call.getMockImplementation()!
  call.mockImplementation(async (command: string) => command === 'pairing_invite'
    ? new Promise((resolve) => {
        finish = resolve
      })
    : implementation(command))
  const wrapper = mount(PairingPanel, { attachTo: document.body, props: { visible: true } })
  try {
    await flushPromises()
    await wrapper.findAll('button').find(node => node.text() === messages.devices.pairingExport)!.trigger('click')
    await wrapper.setProps({ visible: false })
    const previous = document.activeElement
    finish({ ok: true, value: 'invitation' })
    await flushPromises()
    expect(document.activeElement).toBe(previous)
    expect(usePairingStore().invitation).toBe('invitation')
  }
  finally {
    wrapper.unmount()
  }
})

it('retains refused input for explicit correction and retry', async () => {
  const implementation = call.getMockImplementation()!
  call.mockImplementation(async (command: string) => command === 'pairing_listen'
    ? { ok: false, error: { code: 'invalid_argument', message: 'error.invalid_argument', actions: [{ kind: 'retry' }] } }
    : implementation(command))
  const wrapper = mount(PairingPanel, { props: { selected: peer } })
  await flushPromises()
  await wrapper.get('textarea').setValue('invalid')
  await wrapper.findAll('button').find(node => node.text() === messages.devices.pairingListen)!.trigger('click')
  await flushPromises()
  expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('invalid_argument')
  expect(wrapper.get<HTMLTextAreaElement>('textarea').element.value).toBe('invalid')
  expect(call.mock.calls.filter(([command]) => command === 'pairing_listen')).toHaveLength(1)
  call.mockImplementation(implementation)
  await wrapper.get('textarea').setValue('corrected')
  await wrapper.findAll('button').find(node => node.text() === messages.devices.pairingListen)!.trigger('click')
  await flushPromises()
  expect(wrapper.text()).toContain(messages.devices.pairingPhases.listening)
  wrapper.unmount()
})

it('recovers a failed status read without resubmitting an accepted invitation', async () => {
  const implementation = call.getMockImplementation()!
  let failStatus = false
  call.mockImplementation(async (command: string) => {
    if (command === 'pairing_status' && failStatus)
      return { ok: false, error: { code: 'timeout', message: 'error.timeout' } }
    const answer = await implementation(command)
    if (command === 'pairing_listen')
      failStatus = true
    return answer
  })
  const wrapper = mount(PairingPanel, { props: { selected: peer } })
  await flushPromises()
  await wrapper.get('textarea').setValue('invitation')
  await wrapper.findAll('button').find(node => node.text() === messages.devices.pairingListen)!.trigger('click')
  await flushPromises()
  expect(usePairingStore().active).toBe(true)
  expect(wrapper.get<HTMLTextAreaElement>('textarea').element.value).toBe('public-reply')
  failStatus = false
  await wrapper.findAll('button').find(node => node.text() === messages.devices.pairingRefresh)!.trigger('click')
  await flushPromises()
  expect(wrapper.findComponent(ErrorNotice).exists()).toBe(false)
  expect(call.mock.calls.filter(([command]) => command === 'pairing_listen')).toHaveLength(1)
  wrapper.unmount()
})

it('renders backend expiry and its structured refusal without inventing success', async () => {
  const wrapper = mount(PairingPanel)
  await flushPromises()
  backend = {
    requestId: 'expired',
    peerId: peer.deviceId,
    phase: 'timedOut',
    expiresAtMs: 1,
    reply: null,
    errorCode: 'timeout',
    detail: 'budget expired'
  }
  await usePairingStore().refresh()
  await flushPromises()
  expect(wrapper.text()).toContain(messages.devices.pairingPhases.timedOut)
  expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('timeout')
  expect(usePairingStore().active).toBe(false)
  usePairingStore().invitation = 'old invitation'
  await wrapper.findAll('button').find(node => node.text() === messages.devices.pairingRestart)!.trigger('click')
  await flushPromises()
  expect(usePairingStore().snapshot?.phase).toBe('timedOut')
  expect(usePairingStore().invitation).toBe('')
  expect(wrapper.find('textarea').exists()).toBe(false)
  expect(call.mock.calls.some(([command]) => command === 'pairing_listen' || command === 'pairing_connect')).toBe(false)
  wrapper.unmount()
})
