import type { TransferSnapshot } from '@synra/protocol'
import { flushPromises, mount } from '@vue/test-utils'
// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { answer, DEVICES, HomePage, IDENTITY, INTENT, messages, mocks, router, useDevicesStore, useIntentsStore, useNoticesStore } from './index.testkit'

describe('the home page', () => {
  it('seeds the transfer panel from the snapshot, without waiting for an event', async () => {
    const transfer: TransferSnapshot = {
      transferId: '44444444-4444-4444-8444-444444444444',
      peerDevice: '11111111-1111-4111-8111-111111111111',
      peerName: 'Study PC',
      direction: 'receive',
      kind: 'file',
      state: 'active',
      fileName: 'notes.txt',
      receivedBytes: 5,
      totalBytes: 10,
      blocksDone: 0,
      blocksTotal: 1,
      startedAtMs: 1
    }
    answer({ intent_list: [], transfer_list: [transfer] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('transfer_list')
    expect(wrapper.text()).toContain('Study PC')
    expect(wrapper.text()).toContain('notes.txt')
    expect(wrapper.text()).toContain(messages.home.inFlight.receiving)
    expect(wrapper.text()).toContain('50%')
    expect(wrapper.text()).toContain(messages.home.inFlight.states.active)
    const tag = wrapper.findAllComponents({ name: 'ATag' }).find(candidate => candidate.text() === messages.home.inFlight.states.active)
    expect(tag, 'the state tag is not translated').toBeDefined()
    wrapper.unmount()
  })
  it('keeps a transfer state it does not know rather than naming it something else', async () => {
    const transfer: TransferSnapshot = {
      transferId: '55555555-5555-4555-8555-555555555555',
      peerDevice: '11111111-1111-4111-8111-111111111111',
      peerName: 'Study PC',
      direction: 'send',
      kind: 'file',
      state: 'paused',
      fileName: 'notes.txt',
      receivedBytes: 1,
      totalBytes: 10,
      blocksDone: 0,
      blocksTotal: 1,
      startedAtMs: 1
    }
    answer({ intent_list: [], transfer_list: [transfer] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    expect(wrapper.text()).toContain('paused')
    expect(wrapper.text()).not.toContain(messages.home.inFlight.states.active)
    wrapper.unmount()
  })
  it('lists the files already in the inbox', async () => {
    answer({
      intent_list: [],
      transfer_received: [
        { fileName: 'notes.txt', path: 'C:/inbox/notes.txt', bytes: 12, receivedAtMs: 2 },
        { fileName: 'photo.png', path: 'C:/inbox/photo.png', bytes: 2048, receivedAtMs: 1 }
      ]
    })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('transfer_received')
    expect(wrapper.text()).toContain(messages.home.receivedFiles.title)
    expect(wrapper.text()).toContain('notes.txt')
    expect(wrapper.text()).toContain('C:/inbox/notes.txt')
    expect(wrapper.text()).toContain('photo.png')
    expect(wrapper.text()).toContain('2.0 KiB')
    expect(wrapper.findAll('button').some(button => ['Open', 'Show in folder', 'Delete'].includes(button.text()))).toBe(false)
    expect(mocks.call.mock.calls.every(([command]) => ['intent_list', 'transfer_list', 'transfer_received'].includes(command))).toBe(true)
    wrapper.unmount()
  })
  it('re-reads the inbox when a received notice arrives', async () => {
    let reads = 0
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'intent_list' || command === 'transfer_list')
        return { ok: true, value: [] }
      if (command === 'transfer_received') {
        reads += 1
        return {
          ok: true,
          value: reads === 1
            ? []
            : [{ fileName: 'arrived.txt', path: 'C:/inbox/arrived.txt', bytes: 7, receivedAtMs: 3 }]
        }
      }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    expect(wrapper.text()).not.toContain('arrived.txt')
    useNoticesStore().apply({
      type: 'notice',
      payload: { message: 'notice.received', params: { name: 'arrived.txt', path: 'C:/inbox/arrived.txt' } }
    } as never)
    await flushPromises()
    expect(reads).toBe(2)
    expect(wrapper.text()).toContain('arrived.txt')
    wrapper.unmount()
  })
  it('cancels a transfer from its row', async () => {
    const transfer: TransferSnapshot = {
      transferId: '22222222-2222-4222-8222-222222222222',
      peerDevice: '11111111-1111-4111-8111-111111111111',
      peerName: 'Study PC',
      direction: 'receive',
      kind: 'file',
      state: 'active',
      fileName: 'notes.txt',
      receivedBytes: 5,
      totalBytes: 10,
      blocksDone: 0,
      blocksTotal: 1,
      startedAtMs: 1
    }
    answer({ intent_list: [], transfer_cancel: null })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    useIntentsStore().apply({ type: 'transferUpdated', payload: transfer })
    useDevicesStore().applySnapshot({ identity: IDENTITY, devices: DEVICES })
    await flushPromises()
    const connection = wrapper.get(`section[aria-label="${messages.home.wire.label}"]`)
    expect(connection.text()).toContain(transfer.peerName)
    expect(connection.text()).not.toContain(DEVICES[0]!.displayName)
    expect(wrapper.text()).toContain('50%')
    expect(wrapper.text()).toContain('5 B of 10 B')
    const bar = wrapper.get('[role="progressbar"]')
    expect(bar.attributes('aria-valuenow')).toBe('50')
    expect(bar.attributes('aria-valuemin')).toBe('0')
    expect(bar.attributes('aria-valuemax')).toBe('100')
    expect(bar.attributes('aria-valuetext')).toContain('5 B of 10 B')
    const cancel = wrapper.findAll('button').find(button => button.text() === 'Cancel')
    expect(cancel).toBeDefined()
    await cancel?.trigger('click')
    await flushPromises()
    expect(mocks.call).not.toHaveBeenCalledWith('transfer_cancel', expect.anything())
    const question = wrapper.findComponent({ name: 'APopconfirm' })
    expect(question.exists()).toBe(true)
    await question.vm.$emit('confirm')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('transfer_cancel', { transferId: transfer.transferId })
    wrapper.unmount()
  })
  it('cancels at once when the user turned the question off', async () => {
    const transfer: TransferSnapshot = {
      transferId: '33333333-3333-4333-8333-333333333333',
      peerDevice: '11111111-1111-4111-8111-111111111111',
      peerName: 'Study PC',
      direction: 'send',
      kind: 'file',
      state: 'active',
      fileName: 'notes.txt',
      receivedBytes: 0,
      totalBytes: 10,
      blocksDone: 0,
      blocksTotal: 1,
      startedAtMs: 1
    }
    answer({ intent_list: [], transfer_cancel: null })
    const { useSettingsStore } = await import('@/stores/settings')
    useSettingsStore().applySnapshot({ values: { 'transfers.confirmCancel': false }, overridden: [] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    useIntentsStore().apply({ type: 'transferUpdated', payload: transfer })
    await flushPromises()
    expect(wrapper.findComponent({ name: 'APopconfirm' }).exists()).toBe(false)
    const cancel = wrapper.findAll('button').find(button => button.text() === 'Cancel')
    await cancel?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('transfer_cancel', { transferId: transfer.transferId })
    wrapper.unmount()
  })
  it('draws an intent the store received', async () => {
    answer({ intent_list: [] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    useIntentsStore().apply({ type: 'intentUpdated', payload: INTENT })
    await flushPromises()
    expect(wrapper.text()).toContain(INTENT.pluginId)
    const details = wrapper.findAll('button').find(button => button.text() === messages.home.received.details)
    expect(details, 'the details disclosure is missing').toBeDefined()
    await details!.trigger('click')
    expect(wrapper.text()).toContain(INTENT.traceId)
    wrapper.unmount()
  })
  it('drops an intent the kernel says is over', async () => {
    answer({ intent_list: [] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    const store = useIntentsStore()
    store.apply({ type: 'intentUpdated', payload: INTENT })
    await flushPromises()
    expect(wrapper.text()).toContain(INTENT.pluginId)
    store.apply({ type: 'intentFinished', payload: { intentId: INTENT.intentId } })
    await flushPromises()
    expect(wrapper.text()).not.toContain(INTENT.pluginId)
    wrapper.unmount()
  })
})
