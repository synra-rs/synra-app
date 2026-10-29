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
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { setLocalePreference } from '../locale'

const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  subscribe: vi.fn()
}))

vi.mock('../kernel', () => ({ kernel: { call: mocks.call, subscribe: mocks.subscribe } }))

const DevicesPage = (await import('./DevicesPage.vue')).default
const { useDevicesStore } = await import('../stores/devices')
const { DEVICES, IDENTITY } = await import('../ipc/fake-kernel-fixtures')
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

beforeEach(() => {
  installBrowserApis()
  setActivePinia(createPinia())
  mocks.call.mockReset()
  mocks.subscribe.mockReset()
  mocks.subscribe.mockResolvedValue(() => {})
})

describe('the devices page', () => {
  afterEach(() => setLocalePreference('system'))

  it.each(['en', 'zh-CN'])('keeps device management separate from feature launchers in %s', async (language) => {
    setLocalePreference(language)
    const answers: Record<string, unknown> = {
      devices_snapshot: { identity: IDENTITY, devices: DEVICES, scanning: undefined },
      pairing_status: null
    }
    mocks.call.mockImplementation(async (command: string) => command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } })
    const wrapper = mount(DevicesPage)
    try {
      await flushPromises()
      const labels = wrapper.findAll('button').map(button => button.text())
      expect(labels).toContain(messages.devices.forget)
      for (const retired of ['Send text', 'Send file', '发送文字', '发送文件'])
        expect(labels).not.toContain(retired)
      expect(wrapper.findComponent({ name: 'AModal' }).exists()).toBe(false)
      expect(mocks.call.mock.calls.every(([command]) => ['devices_snapshot', 'pairing_status'].includes(command))).toBe(true)
    }
    finally {
      wrapper.unmount()
    }
  })

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
    expect(mocks.call.mock.calls.filter(([command]) => command === 'devices_snapshot')).toHaveLength(1)
    wrapper.unmount()
  })

  it('cancels a running scan by the id the start command answered', async () => {
    // ★ The start command always answered a scan id; the page threw it away, so a scan could only be waited out.
    let snapshots = 0
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'devices_snapshot') {
        snapshots += 1
        return {
          ok: true,
          value: { identity: IDENTITY, devices: [], scanning: snapshots > 1 ? { scanId: 'scan-1', found: 0, elapsedMs: 1, totalMs: 3_000 } : undefined }
        }
      }
      if (command === 'devices_scan_start')
        return { ok: true, value: 'scan-1' }
      if (command === 'devices_scan_cancel')
        return { ok: true, value: null }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })

    const wrapper = mount(DevicesPage)
    await flushPromises()

    const scan = wrapper.findAll('button').find(button => button.text() === messages.devices.scan)
    expect(scan, 'the scan button is missing').toBeDefined()
    await scan?.trigger('click')
    await flushPromises()

    const cancel = wrapper.findAll('button').find(button => button.text() === messages.devices.cancelScan)
    expect(cancel, 'a running scan has no cancel').toBeDefined()
    await cancel?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('devices_scan_cancel', { scanId: 'scan-1' })
    wrapper.unmount()
  })

  it('asks before forgetting a paired device, then calls the command', async () => {
    // ★ Forgetting undoes the expensive step: the next send to that device has to pair again. The confirmation is the
    // page's, and the command is only reached through it — a button that forgot on the first click would be one click
    // away from a decision the user cannot undo.
    mocks.call.mockResolvedValue({ ok: true, value: { identity: IDENTITY, devices: DEVICES, scanning: undefined } })

    const wrapper = mount(DevicesPage)
    await flushPromises()

    const forget = wrapper.findAll('button').find(button => button.text() === messages.devices.forget)
    expect(forget, 'a paired device has no forget button').toBeDefined()

    // ★ The popconfirm is a component, not a DOM assertion: jsdom does not run antd's portal transition, and what this
    // test is about is that the command is behind the confirmation rather than on the first click.
    const confirmation = wrapper.findComponent({ name: 'APopconfirm' })
    expect(confirmation.exists(), 'the confirmation is missing').toBe(true)
    await confirmation.vm.$emit('confirm')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('devices_forget', { deviceId: DEVICES[0]!.deviceId })
    wrapper.unmount()
  })
})
