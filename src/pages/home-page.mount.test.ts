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
})
