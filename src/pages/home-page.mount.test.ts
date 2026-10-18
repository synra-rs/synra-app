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
const { INTENT } = await import('../ipc/fake-kernel-fixtures')

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
  mocks.call.mockImplementation(async (command: string) =>
    command in byCommand
      ? { ok: true, value: byCommand[command] }
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
