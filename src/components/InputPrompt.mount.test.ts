// @vitest-environment jsdom
import type { VueWrapper } from '@vue/test-utils'
import { DOMWrapper, flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { messages } from '../messages'
import { useInputStore } from '../stores/input'
import InputPrompt from './InputPrompt.vue'

const { call } = vi.hoisted(() => ({ call: vi.fn() }))
vi.mock('../kernel', () => ({ kernel: { call } }))
let wrapper: VueWrapper | undefined
const session = { sessionId: 'session', deviceId: 'phone', displayName: '安卓', pluginId: '@synra-plugin/touchpad', phase: 'pending' as const }
beforeEach(() => {
  setActivePinia(createPinia())
  window.matchMedia = vi.fn().mockImplementation(() => ({ matches: false, addListener: vi.fn(), removeListener: vi.fn(), addEventListener: vi.fn(), removeEventListener: vi.fn() }))
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  vi.stubGlobal('IntersectionObserver', class { observe() {} unobserve() {} disconnect() {} })
  call.mockReset().mockResolvedValue({ ok: true, value: null })
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  vi.unstubAllGlobals()
})

it('mounts a global body portal and binds one-time consent to the displayed session', async () => {
  const store = useInputStore()
  wrapper = mount(InputPrompt, { attachTo: document.body })
  store.apply({ type: 'inputUpdated', payload: { session } })
  await flushPromises()
  const body = new DOMWrapper(document.body)
  expect(body.get('[role="dialog"]').text()).toContain('安卓')
  expect(body.get('[role="dialog"]').text()).toContain('@synra-plugin/touchpad')
  await body.findAll('button').find(button => button.text() === messages.input.allow)!.trigger('click')
  await flushPromises()
  expect(call).toHaveBeenCalledWith('devices_input_respond', { sessionId: 'session', allow: true })
})

it('keeps the receiving host stop control visible throughout active input', async () => {
  const store = useInputStore()
  store.apply({ type: 'inputUpdated', payload: { session: { ...session, phase: 'active' } } })
  wrapper = mount(InputPrompt, { attachTo: document.body })
  expect(wrapper.text()).toContain(messages.input.active('安卓'))
  await wrapper.get('button').trigger('click')
  await flushPromises()
  expect(call).toHaveBeenCalledWith('devices_input_stop', { sessionId: 'session' })
})
