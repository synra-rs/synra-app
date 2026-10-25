// @vitest-environment jsdom
import type { PairingSnapshot } from '@synra/protocol'
import { flushPromises, mount } from '@vue/test-utils'
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

it('exports and imports explicitly and restores the reply after navigation', async () => {
  let wrapper = mount(PairingPanel, { props: { selected: peer } })
  await flushPromises()
  const button = (label: string) => wrapper.findAll('button').find(node => node.text() === label)!
  expect(button(messages.devices.pairingListen).attributes('disabled')).toBeDefined()
  await button(messages.devices.pairingExport).trigger('click')
  await flushPromises()
  expect(wrapper.find<HTMLTextAreaElement>('textarea[readonly]').element.value).toBe('public-invitation')
  await wrapper.find('textarea:not([readonly])').setValue(' trusted invitation ')
  await button(messages.devices.pairingListen).trigger('click')
  await flushPromises()
  expect(call).toHaveBeenCalledWith('pairing_listen', { deviceId: peer.deviceId, invitation: 'trusted invitation' })
  expect(wrapper.text()).toContain(messages.devices.pairingPhases.listening)
  expect(button(messages.devices.pairingConnect).attributes('disabled')).toBeDefined()
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
  const second = { ...peer, deviceId: 'another-peer' }
  await wrapper.setProps({ selected: second })
  expect(wrapper.find('textarea').element.value).toBe('')
  await wrapper.find('textarea').setValue('reply')
  await wrapper.findAll('button').find(node => node.text() === messages.devices.pairingConnect)!.trigger('click')
  await flushPromises()
  expect(call).toHaveBeenCalledWith('pairing_connect', { deviceId: second.deviceId, reply: 'reply' })
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
  wrapper.unmount()
})
