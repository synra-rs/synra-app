// @vitest-environment jsdom
import type { VueWrapper } from '@vue/test-utils'
import { DOMWrapper, flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { messages } from '../messages'
import { usePairingStore } from '../stores/pairing'
import PairingPrompt from './PairingPrompt.vue'

const { call } = vi.hoisted(() => ({ call: vi.fn() }))
vi.mock('../app/kernel', () => ({ kernel: { call } }))

let wrapper: VueWrapper | undefined
const peer = { code: '000123', deviceId: 'peer', displayName: '安卓', platform: 'android' }

beforeEach(() => {
  setActivePinia(createPinia())
  window.matchMedia = vi.fn().mockImplementation(() => ({ matches: false, addListener: vi.fn(), removeListener: vi.fn(), addEventListener: vi.fn(), removeEventListener: vi.fn() }))
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  vi.stubGlobal('IntersectionObserver', class { observe() {} unobserve() {} disconnect() {} })
  call.mockReset()
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  vi.unstubAllGlobals()
})

describe('the global pairing prompt', () => {
  it('shows an incoming code with accept refuse and block without mounting the devices page', async () => {
    const store = usePairingStore()
    wrapper = mount(PairingPrompt, { attachTo: document.body })
    store.apply({ type: 'pairingUpdated', payload: { phase: 'asked', ask: peer } })
    await flushPromises()
    expect(new DOMWrapper(document.body).get('output').text()).toBe('000123')
    const buttons = new DOMWrapper(document.body).findAll('button').map(button => button.text())
    expect(buttons).toContain(messages.devices.pairingAccept)
    expect(buttons).toContain(messages.devices.pairingRefuse)
    expect(buttons).toContain(messages.devices.pairingBlock)
    expect(buttons).not.toContain(messages.devices.pairingWithdraw)
    call.mockResolvedValue({ ok: true, value: { phase: 'done', outcome: 'refused' } })
    await new DOMWrapper(document.body).findAll('button').find(button => button.text() === messages.devices.pairingBlock)!.trigger('click')
    await flushPromises()
    expect(call).toHaveBeenCalledWith('devices_pairing_respond', { accepted: false, block: true })
  })

  it('shows the initiating device only the code and cancellation while it waits', async () => {
    const store = usePairingStore()
    store.apply({ type: 'pairingUpdated', payload: { phase: 'waiting', offer: peer } })
    wrapper = mount(PairingPrompt, { attachTo: document.body })
    await flushPromises()
    expect(new DOMWrapper(document.body).get('output').text()).toBe('000123')
    const buttons = new DOMWrapper(document.body).findAll('button').map(button => button.text())
    expect(buttons).toContain(messages.devices.pairingWithdraw)
    expect(buttons).not.toContain(messages.devices.pairingAccept)
    expect(buttons).not.toContain(messages.devices.pairingRefuse)
  })
})
