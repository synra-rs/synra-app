// @vitest-environment jsdom
import type { TransferSnapshot } from '@synra/protocol'
import { flushPromises, mount } from '@vue/test-utils'
import { Checkbox as ACheckbox } from 'ant-design-vue'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { messages } from '../messages'
import { useIntentsStore } from '../stores/intents'
import ErrorNotice from './ErrorNotice.vue'
import TransferPrompt from './TransferPrompt.vue'

const { call } = vi.hoisted(() => ({ call: vi.fn() }))
vi.mock('../kernel', () => ({ kernel: { call } }))

function transfer(id: string, overrides: Partial<TransferSnapshot> = {}): TransferSnapshot {
  return { transferId: id, peerDevice: `peer-${id}`, peerName: `Peer ${id}`, direction: 'receive', kind: 'file', state: 'offered', fileName: `${id}.txt`, receivedBytes: 0, totalBytes: 10, blocksDone: 0, blocksTotal: 1, startedAtMs: 1, ...overrides }
}

function mountPrompt() {
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/:pathMatch(.*)*', component: { template: '<div />' } }] })
  return mount(TransferPrompt, { attachTo: document.body, global: { plugins: [router] } })
}

function offer(...rows: TransferSnapshot[]): void {
  useIntentsStore().applySnapshot({ intents: [], transfers: rows })
}

beforeEach(() => {
  setActivePinia(createPinia())
  call.mockReset()
  call.mockImplementation(async (command: string) => {
    if (command === 'transfer_respond')
      return { ok: true, value: null }
    if (command === 'transfer_list')
      return { ok: true, value: [] }
    throw new Error(command)
  })
})

it('shows only offered incoming files with a count and complete request details', async () => {
  const longName = `${'filename'.repeat(60)}.txt`
  offer(transfer('outgoing', { direction: 'send' }), transfer('first', { fileName: longName }), transfer('second'), transfer('active', { state: 'active' }))
  const wrapper = mountPrompt()
  try {
    expect(wrapper.text()).toContain(messages.transfers.prompt.pending(2))
    expect(wrapper.text()).toContain(longName)
    expect(wrapper.findAll('dd').map(node => node.text())).toContain('peer-first')
    expect(wrapper.findComponent(ACheckbox).props('checked')).toBe(false)
    expect(call).not.toHaveBeenCalled()
  }
  finally { wrapper.unmount() }
})

it.each([true, false])('submits one %s answer and remembers only an explicit acceptance', async (accept) => {
  offer(transfer('first'))
  const wrapper = mountPrompt()
  try {
    await wrapper.findComponent(ACheckbox).vm.$emit('update:checked', true)
    const label = accept ? messages.transfers.prompt.accept : messages.transfers.prompt.reject
    const button = wrapper.findAll('button').find(node => node.text() === label)!
    await button.trigger('click')
    await flushPromises()
    expect(call).toHaveBeenCalledExactlyOnceWith('transfer_respond', { transferId: 'first', accept, remember: accept })
    expect(wrapper.text()).toContain(messages.transfers.prompt.answered)
    expect(wrapper.findAll('button').some(node => node.text() === label)).toBe(false)
    expect(useIntentsStore().transfers).toHaveLength(1)
  }
  finally { wrapper.unmount() }
})

it('keeps a refused request and its choice visible without retrying a different operation', async () => {
  call.mockResolvedValue({ ok: false, error: { code: 'timeout', message: 'error.timeout', retryable: true, actions: [{ kind: 'retry' }] } })
  offer(transfer('first'))
  const wrapper = mountPrompt()
  try {
    await wrapper.findComponent(ACheckbox).vm.$emit('update:checked', true)
    await wrapper.findAll('button').find(node => node.text() === messages.transfers.prompt.accept)!.trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('timeout')
    expect(wrapper.findComponent(ErrorNotice).props('actions')).toEqual([])
    expect(wrapper.findComponent(ACheckbox).props('checked')).toBe(true)
    expect(wrapper.text()).toContain('first.txt')
    expect(wrapper.text()).not.toContain(messages.transfers.prompt.answered)
    await wrapper.findAll('button').find(node => node.text() === messages.transfers.prompt.reject)!.trigger('click')
    await flushPromises()
    expect(call).toHaveBeenLastCalledWith('transfer_respond', { transferId: 'first', accept: false, remember: false })
  }
  finally { wrapper.unmount() }
})

it('clears authorization and a late failure when the queue advances while replying', async () => {
  let finish!: (value: unknown) => void
  call.mockImplementation(() => new Promise((resolve) => {
    finish = resolve
  }))
  offer(transfer('first'), transfer('second'))
  const wrapper = mountPrompt()
  try {
    await wrapper.findComponent(ACheckbox).vm.$emit('update:checked', true)
    const accept = wrapper.findAll('button').find(node => node.text() === messages.transfers.prompt.accept)!
    accept.element.focus()
    await accept.trigger('click')
    await accept.trigger('click')
    expect(call).toHaveBeenCalledTimes(1)
    useIntentsStore().apply({ type: 'transferFinished', payload: { transferId: 'first' } })
    await flushPromises()
    expect(wrapper.text()).toContain('second.txt')
    expect(wrapper.findComponent(ACheckbox).props('checked')).toBe(false)
    expect(document.activeElement).toBe(wrapper.element)
    finish({ ok: false, error: { code: 'timeout', message: 'error.timeout', retryable: true } })
    await flushPromises()
    expect(wrapper.findComponent(ErrorNotice).exists()).toBe(false)
    expect(wrapper.text()).not.toContain(messages.transfers.prompt.answered)
  }
  finally { wrapper.unmount() }
})

it('does not submit an expired request and can read away a stale prompt', async () => {
  offer(transfer('first'))
  const wrapper = mountPrompt()
  try {
    const accept = wrapper.findAll('button').find(node => node.text() === messages.transfers.prompt.accept)!
    useIntentsStore().apply({ type: 'transferFinished', payload: { transferId: 'first' } })
    await accept.trigger('click')
    expect(call).not.toHaveBeenCalled()
    offer(transfer('second'))
    await flushPromises()
    await wrapper.findAll('button').find(node => node.text() === messages.transfers.prompt.accept)!.trigger('click')
    await flushPromises()
    await wrapper.findAll('button').find(node => node.text() === messages.transfers.prompt.checkStatus)!.trigger('click')
    await flushPromises()
    expect(call).toHaveBeenLastCalledWith('transfer_list')
    expect(wrapper.find('section').exists()).toBe(false)
  }
  finally { wrapper.unmount() }
})

it('keeps a status read failure separate and does not resend an accepted response', async () => {
  call.mockImplementation(async (command: string) => command === 'transfer_list'
    ? { ok: false, error: { code: 'timeout', message: 'error.timeout', retryable: true } }
    : { ok: true, value: null })
  offer(transfer('first'))
  const wrapper = mountPrompt()
  try {
    await wrapper.findAll('button').find(node => node.text() === messages.transfers.prompt.accept)!.trigger('click')
    await flushPromises()
    await wrapper.findAll('button').find(node => node.text() === messages.transfers.prompt.checkStatus)!.trigger('click')
    await flushPromises()
    expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('timeout')
    expect(wrapper.text()).toContain(messages.transfers.prompt.answered)
    expect(call.mock.calls.filter(([command]) => command === 'transfer_respond')).toHaveLength(1)
  }
  finally { wrapper.unmount() }
})

it('hands keyboard focus to the next request without grabbing focus on arrival', async () => {
  offer(transfer('first'), transfer('second'))
  const wrapper = mountPrompt()
  try {
    expect(wrapper.element.contains(document.activeElement)).toBe(false)
    const reject = wrapper.findAll('button').find(node => node.text() === messages.transfers.prompt.reject)!
    reject.element.focus()
    useIntentsStore().apply({ type: 'transferFinished', payload: { transferId: 'first' } })
    await flushPromises()
    expect(document.activeElement?.textContent).toBe(messages.transfers.prompt.reject)
    expect(wrapper.text()).toContain('second.txt')
  }
  finally { wrapper.unmount() }
})
