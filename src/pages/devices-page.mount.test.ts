// @vitest-environment jsdom
/**
 * The devices page, mounted.
 *
 * # What is asserted
 *
 * The join between the store and the screen, which is what the last few rounds changed: the page reads
 * `useDevicesStore`, `src/events.ts` writes it, and a card that arrived by event must reach the DOM without the page
 * asking the kernel again. Clicking is not tested — the calls those clicks make are asserted in the command layer.
 *
 * # Why the browser APIs are installed here as well as in the plugins page's test
 *
 * Because each test file gets a fresh jsdom, and a shared setup file would be a second thing to keep in step with the one
 * place that knows which APIs antd needs. Three lines duplicated is cheaper than a helper that hides them.
 */

import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  subscribe: vi.fn()
}))

vi.mock('../kernel', () => ({ kernel: { call: mocks.call, subscribe: mocks.subscribe } }))

const DevicesPage = (await import('./DevicesPage.vue')).default
const { useDevicesStore } = await import('../stores/devices')
const { DEVICES, IDENTITY } = await import('../ipc/fake-kernel-fixtures')

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

beforeEach(() => {
  installBrowserApis()
  setActivePinia(createPinia())
  mocks.call.mockReset()
  mocks.subscribe.mockReset()
  mocks.subscribe.mockResolvedValue(() => {})
})

describe('the devices page', () => {
  it('asks for the snapshot when it is mounted', async () => {
    mocks.call.mockResolvedValue({ ok: true, value: { identity: IDENTITY, devices: [], scanning: undefined } })

    const wrapper = mount(DevicesPage)
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('devices_snapshot')
    wrapper.unmount()
  })

  it('draws a card the store received, without the page asking again', async () => {
    // ★ The point of the store: an event arrives, `events.ts` writes it, and the screen follows. The page never re-reads
    // after the one read at mount, so a card in the store is the only path to this line of text.
    mocks.call.mockResolvedValue({ ok: true, value: { identity: IDENTITY, devices: [], scanning: undefined } })
    const wrapper = mount(DevicesPage)
    await flushPromises()

    useDevicesStore().upsert(DEVICES[0]!)
    await flushPromises()

    expect(wrapper.text()).toContain(DEVICES[0]!.displayName)
    // One read: mount. The card came from the store, not from a second call.
    expect(mocks.call).toHaveBeenCalledTimes(1)
    wrapper.unmount()
  })
})
