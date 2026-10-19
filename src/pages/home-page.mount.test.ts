// @vitest-environment jsdom
/**
 * The home page, mounted.
 *
 * # What is asserted
 *
 * That it asks for the intent list once, and that a card the **store** received reaches the screen — the join the last
 * rounds introduced when the page stopped keeping its own copy of the list.
 *
 * # Why a router
 *
 * The page reads `useRouter()` to offer a link into the devices screen, and mounting without one throws before any
 * assertion runs. A memory router with no routes is enough: nothing here navigates.
 */

import type { TransferSnapshot } from '@synra/protocol'

import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'

const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  subscribe: vi.fn()
}))

vi.mock('../kernel', () => ({ kernel: { call: mocks.call, subscribe: mocks.subscribe } }))

const HomePage = (await import('./HomePage.vue')).default
const { useIntentsStore } = await import('../stores/intents')
const { useNoticesStore } = await import('../stores/notices')
const { INTENT } = await import('../ipc/fake-kernel-fixtures')
const { messages } = await import('../messages')

function installBrowserApis(): void {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false
  })) as unknown as typeof window.matchMedia

  const noop = class {
    observe() {}
    unobserve() {}
    disconnect() {}
    takeRecords() {
      return []
    }
  }
  globalThis.ResizeObserver = noop as unknown as typeof ResizeObserver
  globalThis.IntersectionObserver = noop as unknown as typeof IntersectionObserver
}

/** A router to mount behind. Empty on purpose: nothing in this file navigates. */
function router(): ReturnType<typeof createRouter> {
  return createRouter({ history: createMemoryHistory(), routes: [] })
}

/**
 * Answers by command name.
 *
 * ★ A blanket `{ ok: true, value: [] }` was enough for the plugins page because it makes one call. This page makes more
 * than one, and a value of the wrong shape fails somewhere far from the call that was wrong — which is how the devices
 * page's test first went wrong.
 */
function answer(byCommand: Record<string, unknown>) {
  // ★ `transfer_list` is seeded rather than left to be refused: the page reads both lists on mount by design, and a
  // fixture that refused the second one would put an error notice on every test here — including the ones about intents.
  const answers: Record<string, unknown> = { transfer_list: [], transfer_received: [], ...byCommand }
  mocks.call.mockImplementation(async (command: string) =>
    command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
  )
}

beforeEach(() => {
  installBrowserApis()
  setActivePinia(createPinia())
  mocks.call.mockReset()
  mocks.subscribe.mockReset()
  mocks.subscribe.mockResolvedValue(() => {})
})

describe('the home page', () => {
  it('asks for the intents when it is mounted', async () => {
    answer({ intent_list: [] })

    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('intent_list')
    wrapper.unmount()
  })

  it('dispatches the text the user shares from the desk', async () => {
    // ★ The page listed intents but could not start one. The box is the desktop's share entry; the command is the
    // contract's `share` source, not a page-specific shortcut.
    answer({ intent_list: [], intent_dispatch: '11111111-1111-4111-8111-111111111111' })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()

    const field = wrapper.find(`input[placeholder="${messages.home.share.placeholder}"]`)
    expect(field.exists(), 'the share box is missing').toBe(true)
    await field.setValue('https://example.test/a-link')
    const button = wrapper.findAll('button').find(candidate => candidate.text() === messages.home.share.action)
    await button?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('intent_dispatch', {
      source: { kind: 'share', text: 'https://example.test/a-link', title: null }
    })
    expect((field.element as HTMLInputElement).value).toBe('')
    wrapper.unmount()
  })

  it('shares the clipboard without the page reading it', async () => {
    // ★ The shell reads the clipboard once, for this request. A page that read it itself could look whenever it liked;
    // the command is the whole permission.
    answer({ intent_list: [], intent_dispatch: '11111111-1111-4111-8111-111111111111' })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()

    const button = wrapper.findAll('button').find(candidate => candidate.text() === messages.home.share.clipboard)
    expect(button, 'the share-clipboard button is missing').toBeDefined()
    await button?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('intent_dispatch', { source: { kind: 'clipboard' } })
    wrapper.unmount()
  })

  it('lets the user accept an intent on a candidate device', async () => {
    // ★ A candidate is a decision, not a status line. The page listed intents and never called `intent_decide`, so an
    // intent could arrive and nothing could start it.
    const intent = {
      intentId: '01HQ00000000000000000INTNT1',
      traceId: '01HQ00000000000000000TRACE1',
      state: 'awaiting',
      candidates: [
        { pluginId: '@synra/notes', title: 'Notes', deviceId: '11111111-1111-4111-8111-111111111111', online: true }
      ],
      startedAtMs: 1
    }
    answer({ intent_list: [intent], intent_decide: null })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()

    const accept = wrapper.findAll('button').find(button => button.text() === messages.home.received.accept)
    expect(accept, 'the accept action is missing').toBeDefined()
    await accept?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('intent_decide', {
      intentId: intent.intentId,
      decision: { decision: 'accept', deviceId: intent.candidates[0]!.deviceId }
    })
    wrapper.unmount()
  })

  it('seeds the transfer panel from the snapshot, without waiting for an event', async () => {
    // ★ The gap this closes: a transfer that started before this window opened had no event to arrive on, and the page
    // rendered "nothing on its way" beside a transfer that was running. The first read is what makes the first paint true.
    const transfer: TransferSnapshot = {
      transferId: '44444444-4444-4444-8444-444444444444',
      peerDevice: '11111111-1111-4111-8111-111111111111',
      peerName: 'Study PC',
      direction: 'receive',
      kind: 'file',
      state: 'receiving',
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
    wrapper.unmount()
  })

  it('lists the files already in the inbox', async () => {
    // ★ The in-flight list cannot answer this: its row is removed when the transfer finishes, and the file stays. The
    // history is a read of the inbox, not a remembered table.
    answer({
      intent_list: [],
      transfer_open: null,
      transfer_reveal: null,
      transfer_discard: null,
      transfer_received: [
        { fileName: 'notes.txt', path: 'C:/inbox/notes.txt', bytes: 12, receivedAtMs: 2 },
        { fileName: 'photo.png', path: 'C:/inbox/photo.png', bytes: 2_048, receivedAtMs: 1 }
      ]
    })

    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('transfer_received')
    expect(wrapper.text()).toContain(messages.home.receivedFiles.title)
    expect(wrapper.text()).toContain('notes.txt')
    expect(wrapper.text()).toContain('C:/inbox/notes.txt')

    const open = wrapper.findAll('button').find(button => button.text() === messages.home.receivedFiles.open)
    expect(open, 'a received file has no open action').toBeDefined()
    await open?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('transfer_open', { path: 'C:/inbox/notes.txt' })

    const reveal = wrapper.findAll('button').find(button => button.text() === messages.home.receivedFiles.showInFolder)
    expect(reveal, 'a received file has no show-in-folder action').toBeDefined()
    await reveal?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('transfer_reveal', { path: 'C:/inbox/notes.txt' })

    const question = wrapper.findAllComponents({ name: 'APopconfirm' }).at(0)
    expect(question, 'deleting a received file asks first').toBeDefined()
    await question?.vm.$emit('confirm')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('transfer_discard', { path: 'C:/inbox/notes.txt' })
    expect(wrapper.text()).not.toContain('notes.txt')
    expect(wrapper.text()).toContain('photo.png')
    wrapper.unmount()
  })

  it('re-reads the inbox when a received notice arrives', async () => {
    // ★ The list is a snapshot; the notice is the live fact. Without this, a file received while the page is open gets a
    // banner but not a row, and the next person to look for it finds nothing until a reload.
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
    // ★ One command for both directions: the row does not need to know whether the kernel will stop a worker or leave the
    // identifier for the listener.
    const transfer: TransferSnapshot = {
      transferId: '22222222-2222-4222-8222-222222222222',
      peerDevice: '11111111-1111-4111-8111-111111111111',
      peerName: 'Study PC',
      direction: 'receive',
      kind: 'file',
      state: 'receiving',
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
    await flushPromises()
    // ★ The bar and the sentence are two views of one number: "50%" is what a progress element announces, and the byte
    // count is what a user can quote when the number is wrong.
    expect(wrapper.text()).toContain('50%')
    expect(wrapper.text()).toContain('5 B of 10 B')
    const cancel = wrapper.findAll('button').find(button => button.text() === 'Cancel')
    expect(cancel).toBeDefined()
    await cancel?.trigger('click')
    await flushPromises()

    // ★ Asked first: the click only opens the question, and the bytes are still moving until "Abandon" is pressed.
    expect(mocks.call).not.toHaveBeenCalledWith('transfer_cancel', expect.anything())
    const question = wrapper.findComponent({ name: 'APopconfirm' })
    expect(question.exists()).toBe(true)
    await question.vm.$emit('confirm')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('transfer_cancel', { transferId: transfer.transferId })
    wrapper.unmount()
  })

  it('cancels at once when the user turned the question off', async () => {
    // ★ The confirmation is a preference, not a fixed rule: with `transfers.confirmCancel` false the row's button is
    // itself, and one click is the whole gesture.
    const transfer: TransferSnapshot = {
      transferId: '33333333-3333-4333-8333-333333333333',
      peerDevice: '11111111-1111-4111-8111-111111111111',
      peerName: 'Study PC',
      direction: 'send',
      kind: 'file',
      state: 'sending',
      fileName: 'notes.txt',
      receivedBytes: 0,
      totalBytes: 10,
      blocksDone: 0,
      blocksTotal: 1,
      startedAtMs: 1
    }
    answer({ intent_list: [], transfer_cancel: null })
    const { useSettingsStore } = await import('../stores/settings')
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
    // ★ The page reads `useIntentsStore`; `src/events.ts` writes it. An intent put in the store after mount still has to
    // reach the screen, or the two halves are only connected by the read at startup.
    answer({ intent_list: [] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()

    useIntentsStore().apply({ type: 'intentUpdated', payload: INTENT })
    await flushPromises()

    // The fixture's one human word is the plugin it was routed to; `IntentSnapshot` carries no summary field, and an
    // assertion against a field the DTO does not have would pass on `undefined`.
    expect(wrapper.text()).toContain(INTENT.pluginId)
    // ★ And the trace id, which is what a user pastes into `synra logs --trace`. It was on the board and missing from
    // the snapshot, so the filter had no id anyone could know.
    expect(wrapper.text()).toContain(INTENT.traceId)
    wrapper.unmount()
  })

  it('drops an intent the kernel says is over', async () => {
    // ★ `intentFinished` is the one intent event that removes rather than replaces. A page that only handled
    // `intentUpdated` would keep a finished intent on screen — the row the user is most likely to notice is wrong.
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
