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
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setLocalePreference } from '../locale'
import { formatTime } from '../locale-format'

const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  subscribe: vi.fn()
}))

vi.mock('../kernel', () => ({ kernel: { call: mocks.call, subscribe: mocks.subscribe } }))

const HomePage = (await import('./HomePage.vue')).default
const { useIntentsStore } = await import('../stores/intents')
const { useDevicesStore } = await import('../stores/devices')
const { useNoticesStore } = await import('../stores/notices')
const { INTENT, DEVICES, IDENTITY } = await import('../ipc/fake-kernel-fixtures')
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

afterEach(() => setLocalePreference('system'))

describe('the home page', () => {
  it.each(['en', 'zh-CN'])('shows activity data without built-in feature launchers in %s', async (language) => {
    setLocalePreference(language)
    answer({ intent_list: [INTENT] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    try {
      await flushPromises()
      expect(wrapper.text()).toContain(messages.home.received.title)
      expect(wrapper.text()).toContain(messages.home.received.states.succeeded)
      expect(wrapper.text()).toContain('@synra-plugin/github')
      // [05 §5](../../design-docs/05-home-and-activity.md): the trace need not be open on every row, but it must be
      // readable in full once the row is opened — so the collapsed state is asserted, then the opened one.
      expect(wrapper.text()).not.toContain('01HQ00000000000000000TRACE1')
      const details = wrapper.findAll('button').find(button => button.text() === messages.home.received.details)
      expect(details).toBeDefined()
      await details!.trigger('click')
      expect(wrapper.text()).toContain('01HQ00000000000000000TRACE1')
      const labels = wrapper.findAll('button').map(button => button.text())
      for (const retired of ['Share', 'Share clipboard', '分享', '分享剪贴板'])
        expect(labels).not.toContain(retired)
      expect(wrapper.findComponent({ name: 'AInput' }).exists()).toBe(false)
      expect(mocks.call.mock.calls.every(([command]) => ['intent_list', 'transfer_list', 'transfer_received'].includes(command))).toBe(true)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('asks for the intents when it is mounted', async () => {
    answer({ intent_list: [] })

    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('intent_list')
    wrapper.unmount()
  })

  it('shows the four facts a received file row owes', async () => {
    // ★ [05 §3](../../design-docs/05-home-and-activity.md) lists name, size and **received time** as the row's default
    // content. The time was in the DTO from the start and the row never showed it, so a person could see that a file
    // arrived but not when.
    const receivedAtMs = Date.UTC(2026, 9, 30, 4, 5, 6)
    // A path long enough that clipping it would hide where the file actually is.
    const path = 'C:\\Users\\someone\\AppData\\Roaming\\Synra\\inbox\\a-rather-long-folder-name\\notes.md'
    answer({
      intent_list: [],
      transfer_received: [
        { fileName: 'notes.md', path, bytes: 2048, receivedAtMs }
      ]
    })

    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()
    try {
      const text = wrapper.text()
      expect(text).toContain('notes.md')
      expect(text).toContain(path)
      expect(text).toContain('2.0 KiB')
      // The formatted time, not the epoch: a raw millisecond count is not a time a person reads.
      expect(text).toContain(messages.home.receivedFiles.receivedAt)
      expect(text).not.toContain(String(receivedAtMs))

      // ★ `text()` returns the whole string whether or not CSS clips it, so the assertion that matters is on the
      // class list: `truncate` would put an ellipsis in the middle of the one value a person needs to open the file,
      // and this row has no detail view to open instead.
      const shown = wrapper.findAll('span').find(span => span.text() === path)
      expect(shown, 'the full path is not rendered').toBeDefined()
      expect(shown!.classes()).not.toContain('truncate')
      expect(shown!.attributes('title')).toBe(path)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('lets the user accept an intent on a candidate device', async () => {
    // ★ A candidate is a decision, not a status line. The page listed intents and never called `intent_decide`, so an
    // intent could arrive and nothing could start it.
    const intent = {
      intentId: '01HQ00000000000000000INTNT1',
      traceId: '01HQ00000000000000000TRACE1',
      state: 'choosing',
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
    // ★ The row truncates this line, and `deviceName` falls back to the id when the device is not in the store — so on
    // an unregistered device the clipped id is the only text naming it. The `title` is where the whole value remains.
    const idShown = wrapper.findAll('span').find(span => span.attributes('title') === intent.candidates[0]!.deviceId)
    expect(idShown, 'the candidate id has no title').toBeDefined()
    await accept?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('intent_decide', {
      intentId: intent.intentId,
      decision: { decision: 'accept', deviceId: intent.candidates[0]!.deviceId }
    })
    wrapper.unmount()
  })

  it('lets the user decline an intent while it is still matching', async () => {
    // ★ With no candidates yet there is no Accept, but the share is still the user's to refuse. The old rule tied
    // Decline to "has candidates", so an intent still matching had no action at all. A *finished* intent (`noHandler`)
    // is the one that must not offer Decline.
    const intent = {
      intentId: '01HQ00000000000000000INTNT2',
      traceId: '01HQ00000000000000000TRACE2',
      state: 'matching',
      candidates: [],
      startedAtMs: 1
    }
    answer({ intent_list: [intent], intent_decide: null })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()

    expect(wrapper.text()).toContain(messages.home.received.states.matching)
    const decline = wrapper.findAll('button').find(button => button.text() === messages.home.received.decline)
    expect(decline, 'a share still matching cannot be refused').toBeDefined()
    await decline?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('intent_decide', {
      intentId: intent.intentId,
      decision: { decision: 'decline', reason: null }
    })
    wrapper.unmount()
  })

  it('says how many times a dispatch has been retried, and when it gave up', async () => {
    // ★ The kernel retries a dispatch nobody answers; without this line the card said "Running" through all of it, and
    // then went on saying it after the retry bound was reached.
    const nextAtMs = Date.UTC(2026, 9, 22, 10, 42)
    const retrying = {
      intentId: '01HQ00000000000000000INTNT4',
      traceId: '01HQ00000000000000000TRACE4',
      state: 'dispatching',
      candidates: [],
      retry: { attempts: 3, maxAttempts: 8, gaveUp: false, nextAtMs },
      startedAtMs: 1
    }
    const gaveUp = {
      intentId: '01HQ00000000000000000INTNT5',
      traceId: '01HQ00000000000000000TRACE5',
      state: 'dispatching',
      candidates: [],
      retry: { attempts: 8, maxAttempts: 8, gaveUp: true },
      startedAtMs: 1
    }
    answer({ intent_list: [retrying, gaveUp] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()

    const clock = formatTime(nextAtMs, true)
    expect(wrapper.text()).toContain(`Retrying (3/8), next try at ${clock}`)
    expect(wrapper.text()).toContain(messages.home.received.noAnswer)
    wrapper.unmount()
  })

  it('says why an intent failed without printing the kernel key', async () => {
    // ★ `IntentSnapshot.error` carries a code and a message **key**, not a sentence. A page that printed either would put
    // an engine word on screen; the fallback chain is the one `ErrorNotice` already uses.
    const intent = {
      intentId: '01HQ00000000000000000INTNT3',
      traceId: '01HQ00000000000000000TRACE3',
      state: 'failed',
      candidates: [],
      error: { code: 'plugin_faulted', message: 'error.somethingUnmapped', retryable: false },
      startedAtMs: 1
    }
    answer({ intent_list: [intent] })
    const wrapper = mount(HomePage, { global: { plugins: [router()] } })
    await flushPromises()

    expect(wrapper.text()).toContain(messages.errors.plugin_faulted)
    expect(wrapper.text()).not.toContain('error.somethingUnmapped')
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

    expect(wrapper.text()).toContain('photo.png')
    expect(wrapper.text()).toContain('2.0 KiB')
    expect(wrapper.findAll('button').some(button => ['Open', 'Show in folder', 'Delete'].includes(button.text()))).toBe(false)
    expect(mocks.call.mock.calls.every(([command]) => ['intent_list', 'transfer_list', 'transfer_received'].includes(command))).toBe(true)
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
    useDevicesStore().applySnapshot({ identity: IDENTITY, devices: DEVICES })
    await flushPromises()
    // A peer missing from discovery still has a transfer name; do not show the first reachable device instead.
    const connection = wrapper.get(`section[aria-label="${messages.home.wire.label}"]`)
    expect(connection.text()).toContain(transfer.peerName)
    expect(connection.text()).not.toContain(DEVICES[0]!.displayName)
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
    // the snapshot, so the filter had no id anyone could know. It sits behind the row's disclosure now, so the check
    // opens it the way a person would.
    const details = wrapper.findAll('button').find(button => button.text() === messages.home.received.details)
    expect(details, 'the details disclosure is missing').toBeDefined()
    await details!.trigger('click')
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
