// @vitest-environment jsdom
/**
 * The devices page, mounted.
 *
 * # What is asserted
 *
 * The join between the store and the screen, which is what the last few rounds changed: the page reads
 * `useDevicesStore`, `src/events.ts` writes it, and a card that arrived by event must reach the DOM without the page
 * asking the kernel again. Command tests cover the kernel; these tests cover confirmation, focus, refusal and races
 * at the page's production entry points.
 *
 * # Why the browser APIs are installed here as well as in the plugins page's test
 *
 * Because each test file gets a fresh jsdom, and a shared setup file would be a second thing to keep in step with the one
 * place that knows which APIs antd needs. Three lines duplicated is cheaper than a helper that hides them.
 */

import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import ErrorNotice from '../components/ErrorNotice.vue'
import { setLocalePreference } from '../locale'

const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  subscribe: vi.fn()
}))

vi.mock('../kernel', () => ({ kernel: { call: mocks.call, subscribe: mocks.subscribe } }))

const DevicesPage = (await import('./DevicesPage.vue')).default
const { useDevicesStore } = await import('../stores/devices')

/**
 * Mounts the page with a router.
 *
 * ★ The page reads `?device=` to open the pairing panel for a device a notice named, so it needs a route to exist. Every
 * case here starts at `/devices` with no query, which is the same state as arriving from the nav.
 */
async function mountDevices(options: { attachTo?: HTMLElement, query?: Record<string, string> } = {}) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: '/devices', component: { template: '<div />' } }]
  })
  // Awaited: memory history resolves the first location asynchronously; without this the page still reads `/`.
  await router.push({ path: '/devices', query: options.query ?? {} })
  const global = { plugins: [router] }
  return options.attachTo
    ? mount(DevicesPage, { attachTo: options.attachTo, global })
    : mount(DevicesPage, { global })
}
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

  it('opens pairing on demand and keeps the selected invitation draft when hidden', async () => {
    const answers: Record<string, unknown> = {
      devices_snapshot: { identity: IDENTITY, devices: DEVICES },
      pairing_status: null
    }
    mocks.call.mockImplementation(async (command: string) => command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } })
    const wrapper = await mountDevices({ attachTo: document.body })
    try {
      await flushPromises()
      expect(wrapper.get('#device-pairing').isVisible()).toBe(false)
      await wrapper.findAll('button').find(button => button.text() === messages.devices.pair)?.trigger('click')
      expect(wrapper.get('#device-pairing').isVisible()).toBe(true)
      await wrapper.get('#device-pairing textarea').setValue('invitation-draft')
      await flushPromises()
      const toggle = wrapper.get('[aria-controls="device-pairing"]')
      await toggle.trigger('click')
      await flushPromises()
      expect(wrapper.get('#device-pairing').isVisible()).toBe(false)
      await toggle.trigger('click')
      await flushPromises()
      expect((wrapper.get('#device-pairing textarea').element as HTMLTextAreaElement).value).toBe('invitation-draft')
      expect(mocks.call.mock.calls.map(([command]) => command).sort()).toEqual(['devices_snapshot', 'pairing_status'])
    }
    finally {
      wrapper.unmount()
    }
  })

  it('opens pairing for the device a notice named', async () => {
    // ★ `ErrorNotice` links here as `/devices?device=<id>`. Without reading it the button dropped the reader at the top
    // of the list and left them to find the device the error had already named.
    const answers: Record<string, unknown> = {
      devices_snapshot: { identity: IDENTITY, devices: DEVICES, scanning: undefined },
      pairing_status: { state: 'idle' }
    }
    mocks.call.mockImplementation(async (command: string) => command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } })
    // `DEVICES[0]` is already paired, so the named device is one the list offers to pair.
    const wrapper = await mountDevices({ attachTo: document.body, query: { device: DEVICES[1]!.deviceId } })
    try {
      await flushPromises()
      expect(wrapper.get('#device-pairing').isVisible()).toBe(true)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('drops the device from the URL when pairing is closed', async () => {
    // ★ `PluginsPage` drops `?plugin=` when the reader leaves the row (`leaveTarget`). Without the same here, closing
    // the panel left the id in the address bar, so navigating away and back re-opened a panel already dismissed.
    const answers: Record<string, unknown> = {
      devices_snapshot: { identity: IDENTITY, devices: DEVICES, scanning: undefined },
      pairing_status: { state: 'idle' }
    }
    mocks.call.mockImplementation(async (command: string) => command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } })
    const wrapper = await mountDevices({ attachTo: document.body, query: { device: DEVICES[1]!.deviceId } })
    try {
      await flushPromises()
      expect(wrapper.get('#device-pairing').isVisible()).toBe(true)
      await wrapper.get('[aria-controls="device-pairing"]').trigger('click')
      await flushPromises()
      expect(wrapper.get('#device-pairing').isVisible()).toBe(false)
      const router = (wrapper.vm as unknown as {
        $router: { currentRoute: { value: { query: Record<string, unknown> } } }
      }).$router
      expect(router.currentRoute.value.query.device).toBeUndefined()
    }
    finally {
      wrapper.unmount()
    }
  })

  it('stays shut when the named device is not in the snapshot', async () => {
    // An id the list does not carry would select a peer nothing can resolve, and the panel would open empty.
    const answers: Record<string, unknown> = {
      devices_snapshot: { identity: IDENTITY, devices: [], scanning: undefined },
      pairing_status: { state: 'idle' }
    }
    mocks.call.mockImplementation(async (command: string) => command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } })
    const wrapper = await mountDevices({ query: { device: 'not-in-the-list' } })
    try {
      await flushPromises()
      expect(wrapper.get('#device-pairing').isVisible()).toBe(false)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('reveals a pairing task restored from the shared service', async () => {
    const answers: Record<string, unknown> = {
      devices_snapshot: { identity: IDENTITY, devices: DEVICES },
      pairing_status: { requestId: 'restored', peerId: DEVICES[1]!.deviceId, phase: 'listening', expiresAtMs: Date.now() + 60_000, reply: 'reply', errorCode: null, detail: null }
    }
    mocks.call.mockImplementation(async (command: string) => command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } })
    const wrapper = await mountDevices({ attachTo: document.body })
    try {
      await flushPromises()
      expect(wrapper.get('#device-pairing').isVisible()).toBe(true)
      expect(wrapper.text()).toContain(messages.devices.pairingPhases.listening)
      expect(wrapper.get('[aria-controls="device-pairing"]').text()).toBe(messages.devices.pairingResume)
    }
    finally {
      wrapper.unmount()
    }
  })

  it.each(['en', 'zh-CN'])('keeps device management separate from feature launchers in %s', async (language) => {
    setLocalePreference(language)
    const answers: Record<string, unknown> = {
      devices_snapshot: { identity: IDENTITY, devices: DEVICES, scanning: undefined },
      pairing_status: null
    }
    mocks.call.mockImplementation(async (command: string) => command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } })
    const wrapper = await mountDevices()
    try {
      await flushPromises()
      const labels = wrapper.findAll('button').map(button => button.text())
      expect(labels).not.toContain(messages.devices.forget)
      await wrapper.findAll('button').find(button => button.attributes('aria-label') === messages.devices.detailsFor(DEVICES[0]!.displayName))!.trigger('click')
      expect(wrapper.findAll('button').map(button => button.text())).toContain(messages.devices.forget)
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

    const wrapper = await mountDevices()
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('devices_snapshot')
    wrapper.unmount()
  })

  it('draws a card the store received, without the page asking again', async () => {
    // ★ The point of the store: an event arrives, `events.ts` writes it, and the screen follows. The page never re-reads
    // after the one read at mount, so a card in the store is the only path to this line of text.
    mocks.call.mockResolvedValue({ ok: true, value: { identity: IDENTITY, devices: [], scanning: undefined } })
    const wrapper = await mountDevices()
    await flushPromises()

    useDevicesStore().upsert(DEVICES[0]!)
    await flushPromises()

    expect(wrapper.text()).toContain(DEVICES[0]!.displayName)
    // One read: mount. The card came from the store, not from a second call.
    expect(mocks.call.mock.calls.filter(([command]) => command === 'devices_snapshot')).toHaveLength(1)
    wrapper.unmount()
  })

  it('says the last-seen time is unknown instead of showing the epoch', async () => {
    // ★ `DeviceRow` prints `lastSeenMs` when it is above zero and `unknownTime` otherwise. The fixture's first card
    // carries `lastSeenMs: 0`, so this branch has always been drawn — it was just never asserted. Without the guard the
    // page would print 1970 for a device it has never had a session with, which reads as a real timestamp.
    mocks.call.mockResolvedValue({ ok: true, value: { identity: IDENTITY, devices: [DEVICES[0]!], scanning: undefined } })
    const wrapper = await mountDevices()
    await flushPromises()

    const details = wrapper.findAll('button').find(button => button.text() === messages.devices.details)
    expect(details, 'the details toggle is missing').toBeDefined()
    await details!.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain(messages.devices.unknownTime)
    expect(wrapper.text()).not.toContain('1970')
    wrapper.unmount()
  })

  it('cancels a running scan by the id in the authoritative snapshot', async () => {
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

    const wrapper = await mountDevices()
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
    mocks.call.mockImplementation(async (command: string) => ({
      ok: true,
      value: command === 'devices_snapshot' ? { identity: IDENTITY, devices: DEVICES } : null
    }))

    const wrapper = await mountDevices()
    await flushPromises()

    await wrapper.findAll('button').find(button => button.attributes('aria-label') === messages.devices.detailsFor(DEVICES[0]!.displayName))!.trigger('click')
    await wrapper.findAll('button').find(button => button.text() === messages.devices.forget)!.trigger('click')
    await flushPromises()
    expect(mocks.call).not.toHaveBeenCalledWith('devices_forget', expect.anything())
    expect(wrapper.get('[role="dialog"]').text()).toContain(messages.devices.forgetTitle(DEVICES[0]!.displayName))
    await wrapper.get('[role="dialog"]').findAll('button').find(button => button.text() === messages.devices.forget)!.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('devices_forget', { deviceId: DEVICES[0]!.deviceId })
    wrapper.unmount()
  })

  it('groups device relationships and reveals technical facts only in details', async () => {
    mocks.call.mockImplementation(async (command: string) => ({ ok: true, value: command === 'devices_snapshot' ? { identity: IDENTITY, devices: DEVICES } : null }))
    const wrapper = await mountDevices()
    await flushPromises()
    const paired = wrapper.get(`section[aria-label="${messages.devices.paired}"]`)
    const nearby = wrapper.get(`section[aria-label="${messages.devices.nearby}"]`)
    expect(paired.text()).toContain(DEVICES[0]!.displayName)
    expect(nearby.text()).toContain(DEVICES[1]!.displayName)
    expect(paired.text()).not.toContain(DEVICES[1]!.displayName)
    expect(nearby.text()).not.toContain(DEVICES[1]!.deviceId)
    expect(nearby.text()).not.toContain(DEVICES[1]!.address)
    await nearby.get('[aria-expanded]').trigger('click')
    expect(nearby.text()).toContain(DEVICES[1]!.deviceId)
    expect(nearby.text()).toContain(DEVICES[1]!.address)
    useDevicesStore().upsert({ ...DEVICES[1]!, blocked: true, displayName: 'Renamed device' })
    await flushPromises()
    expect(nearby.text()).toContain(messages.devices.blocked)
    expect(nearby.get('[aria-expanded]').attributes('aria-expanded')).toBe('true')
    expect(nearby.text()).toContain('Renamed device')
    wrapper.unmount()
  })

  it('retains the confirmation and paired state after refusal and prevents duplicate submissions', async () => {
    let finish!: (value: unknown) => void
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'devices_snapshot')
        return { ok: true, value: { identity: IDENTITY, devices: DEVICES } }
      if (command === 'devices_forget') {
        return new Promise((resolve) => {
          finish = resolve
        })
      }
      return { ok: true, value: null }
    })
    const wrapper = await mountDevices({ attachTo: document.body })
    try {
      await flushPromises()
      await wrapper.findAll('button').find(button => button.attributes('aria-label') === messages.devices.detailsFor(DEVICES[0]!.displayName))!.trigger('click')
      const trigger = wrapper.findAll('button').find(button => button.text() === messages.devices.forget)!
      trigger.element.focus()
      await trigger.trigger('click')
      await flushPromises()
      const dialog = wrapper.get('[role="dialog"]')
      const keep = dialog.findAll('button').find(button => button.text() === messages.devices.forgetCancel)!
      expect(document.activeElement).toBe(keep.element)
      const confirm = dialog.findAll('button').find(button => button.text() === messages.devices.forget)!
      await confirm.trigger('click')
      await confirm.trigger('click')
      await keep.trigger('click')
      expect(mocks.call.mock.calls.filter(([command]) => command === 'devices_forget')).toHaveLength(1)
      expect(wrapper.get('[role="dialog"]').isVisible()).toBe(true)
      expect(keep.attributes('disabled')).toBeDefined()
      finish({ ok: false, error: { code: 'timeout', message: 'error.timeout', retryable: true } })
      await flushPromises()
      expect(wrapper.get('[role="dialog"]').findComponent(ErrorNotice).props('code')).toBe('timeout')
      expect(useDevicesStore().get(DEVICES[0]!.deviceId)?.paired).toBe(true)
      await keep.trigger('click')
      await flushPromises()
      expect(document.activeElement).toBe(trigger.element)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('keeps a successful removal when refreshing the remaining devices fails', async () => {
    let removed = false
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'devices_snapshot') {
        return removed
          ? { ok: false, error: { code: 'timeout', message: 'error.timeout', retryable: true } }
          : { ok: true, value: { identity: IDENTITY, devices: DEVICES } }
      }
      if (command === 'devices_forget')
        removed = true
      return { ok: true, value: null }
    })
    const wrapper = await mountDevices()
    await flushPromises()
    await wrapper.findAll('button').find(button => button.attributes('aria-label') === messages.devices.detailsFor(DEVICES[0]!.displayName))!.trigger('click')
    await wrapper.findAll('button').find(button => button.text() === messages.devices.forget)!.trigger('click')
    await flushPromises()
    await wrapper.get('[role="dialog"]').findAll('button').find(button => button.text() === messages.devices.forget)!.trigger('click')
    await flushPromises()
    expect(useDevicesStore().get(DEVICES[0]!.deviceId)).toBeUndefined()
    expect(wrapper.text()).toContain(messages.devices.forgotten(DEVICES[0]!.displayName))
    expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('timeout')
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('can stop a scan restored from the store without starting a new scan', async () => {
    let running = true
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'devices_snapshot') {
        return { ok: true, value: { identity: IDENTITY, devices: DEVICES, scanning: running ? { scanId: 'restored-scan', found: 2, elapsedMs: 200, totalMs: 1000 } : undefined } }
      }
      if (command === 'devices_scan_cancel')
        running = false
      return { ok: true, value: null }
    })
    const wrapper = await mountDevices()
    await flushPromises()
    await wrapper.findAll('button').find(button => button.text() === messages.devices.cancelScan)!.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('devices_scan_cancel', { scanId: 'restored-scan' })
    expect(mocks.call.mock.calls.some(([command]) => command === 'devices_scan_start')).toBe(false)
    expect(wrapper.text()).toContain(messages.devices.scanStopped)
    wrapper.unmount()
  })

  it('reports cached fallback separately from a successful network scan', async () => {
    mocks.call.mockImplementation(async (command: string, args?: { opts?: { cachedOnly?: boolean } }) => {
      if (command === 'devices_snapshot')
        return { ok: true, value: { identity: IDENTITY, devices: DEVICES } }
      if (command === 'devices_scan_start') {
        return args?.opts?.cachedOnly
          ? { ok: true, value: 'cached' }
          : { ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } }
      }
      return { ok: true, value: null }
    })
    const wrapper = await mountDevices()
    await flushPromises()
    await wrapper.findAll('button').find(button => button.text() === messages.devices.scan)!.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain(messages.devices.scanCached)
    expect(wrapper.text()).not.toContain(messages.devices.scanFinished)
    expect(wrapper.text()).toContain(DEVICES[0]!.displayName)
    wrapper.unmount()
  })

  it('shows a read failure without pretending the device list is empty and can recover', async () => {
    let failed = true
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'devices_snapshot') {
        return failed
          ? { ok: false, error: { code: 'timeout', message: 'error.timeout', retryable: true } }
          : { ok: true, value: { identity: IDENTITY, devices: DEVICES } }
      }
      return { ok: true, value: null }
    })
    const wrapper = await mountDevices()
    try {
      await flushPromises()
      expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('timeout')
      expect(wrapper.text()).not.toContain(messages.devices.empty)
      expect(wrapper.text()).not.toContain(messages.devices.scanFinished)
      failed = false
      await wrapper.findAll('button').find(button => button.text() === messages.devices.refresh)!.trigger('click')
      await flushPromises()
      expect(wrapper.findComponent(ErrorNotice).exists()).toBe(false)
      expect(wrapper.text()).toContain(DEVICES[0]!.displayName)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('does not report scan success if reading its result fails', async () => {
    let scanned = false
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'devices_snapshot') {
        return scanned
          ? { ok: false, error: { code: 'timeout', message: 'error.timeout', retryable: true } }
          : { ok: true, value: { identity: IDENTITY, devices: DEVICES } }
      }
      if (command === 'devices_scan_start') {
        scanned = true
        return { ok: true, value: 'scan' }
      }
      return { ok: true, value: null }
    })
    const wrapper = await mountDevices()
    try {
      await flushPromises()
      await wrapper.findAll('button').find(button => button.text() === messages.devices.scan)!.trigger('click')
      await flushPromises()
      expect(wrapper.text()).not.toContain(messages.devices.scanFinished)
      expect(wrapper.text()).toContain(messages.devices.knownOnly)
      expect(wrapper.text()).toContain(DEVICES[0]!.displayName)
      expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('timeout')
    }
    finally {
      wrapper.unmount()
    }
  })

  it('can request cancellation before start returns and waits for the pending call', async () => {
    let finish!: (value: unknown) => void
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'devices_snapshot')
        return { ok: true, value: { identity: IDENTITY, devices: DEVICES } }
      if (command === 'devices_scan_start') {
        return new Promise((resolve) => {
          finish = resolve
        })
      }
      return { ok: true, value: null }
    })
    const wrapper = await mountDevices()
    try {
      await flushPromises()
      await wrapper.findAll('button').find(button => button.text() === messages.devices.scan)!.trigger('click')
      useDevicesStore().applySnapshot({ identity: IDENTITY, devices: DEVICES, scanning: { scanId: 'pending-scan', found: 0, elapsedMs: 0, totalMs: 1000 } })
      await flushPromises()
      await wrapper.findAll('button').find(button => button.text() === messages.devices.cancelScan)!.trigger('click')
      await flushPromises()
      expect(mocks.call).toHaveBeenCalledWith('devices_scan_cancel', { scanId: 'pending-scan' })
      expect(wrapper.text()).toContain(messages.devices.scanStopping)
      const scan = wrapper.findAll('button').find(button => button.text() === messages.devices.scanning)!
      expect(scan.attributes('disabled')).toBeDefined()
      finish({ ok: true, value: 'pending-scan' })
      await flushPromises()
      expect(wrapper.text()).toContain(messages.devices.scanStopped)
      expect(wrapper.text()).not.toContain(messages.devices.scanFinished)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('waits for actual completion and does not carry a stop into a replacement scan', async () => {
    let scanId: string | undefined = 'original-scan'
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'devices_snapshot')
        return { ok: true, value: { identity: IDENTITY, devices: DEVICES, scanning: scanId ? { scanId, found: 0, elapsedMs: 0, totalMs: 1000 } : undefined } }
      return { ok: true, value: null }
    })
    const wrapper = await mountDevices()
    try {
      await flushPromises()
      await wrapper.findAll('button').find(button => button.text() === messages.devices.cancelScan)!.trigger('click')
      await flushPromises()
      expect(wrapper.text()).toContain(messages.devices.scanStopping)
      expect(wrapper.text()).not.toContain(messages.devices.scanStopped)
      scanId = 'replacement-scan'
      useDevicesStore().applySnapshot({ identity: IDENTITY, devices: DEVICES, scanning: { scanId, found: 0, elapsedMs: 0, totalMs: 1000 } })
      await flushPromises()
      expect(wrapper.text()).not.toContain(messages.devices.scanStopping)
      scanId = undefined
      useDevicesStore().applySnapshot({ identity: IDENTITY, devices: DEVICES })
      await flushPromises()
      expect(wrapper.text()).toContain(messages.devices.scanFinished)
      expect(wrapper.text()).not.toContain(messages.devices.scanStopped)
    }
    finally {
      wrapper.unmount()
    }
  })

  it.each([false, true])('ignores an earlier cancellation even if the replacement has finished: %s', async (replacementFinished) => {
    let scanId: string | undefined = 'original-scan'
    let finish!: (value: unknown) => void
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'devices_snapshot')
        return { ok: true, value: { identity: IDENTITY, devices: DEVICES, scanning: scanId ? { scanId, found: 0, elapsedMs: 0, totalMs: 1000 } : undefined } }
      if (command === 'devices_scan_cancel')
        return new Promise((resolve) => { finish = resolve })
      return { ok: true, value: null }
    })
    const wrapper = await mountDevices()
    try {
      await flushPromises()
      await wrapper.findAll('button').find(button => button.text() === messages.devices.cancelScan)!.trigger('click')
      scanId = 'replacement-scan'
      useDevicesStore().applySnapshot({ identity: IDENTITY, devices: DEVICES, scanning: { scanId, found: 0, elapsedMs: 0, totalMs: 1000 } })
      await flushPromises()
      if (replacementFinished) {
        scanId = undefined
        useDevicesStore().applySnapshot({ identity: IDENTITY, devices: DEVICES })
        await flushPromises()
      }
      finish({ ok: true, value: null })
      await flushPromises()
      expect(wrapper.text()).not.toContain(messages.devices.scanStopping)
      expect(wrapper.text()).not.toContain(messages.devices.scanStopped)
      expect(useDevicesStore().scan?.scanId).toBe(replacementFinished ? undefined : 'replacement-scan')
    }
    finally {
      wrapper.unmount()
    }
  })

  it('never retries a failed cancellation against a different scan', async () => {
    let scanId = 'original-scan'
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'devices_snapshot')
        return { ok: true, value: { identity: IDENTITY, devices: DEVICES, scanning: { scanId, found: 0, elapsedMs: 0, totalMs: 1000 } } }
      if (command === 'devices_scan_cancel')
        return { ok: false, error: { code: 'timeout', message: 'error.timeout', retryable: true } }
      return { ok: true, value: null }
    })
    const wrapper = await mountDevices()
    try {
      await flushPromises()
      await wrapper.findAll('button').find(button => button.text() === messages.devices.cancelScan)!.trigger('click')
      await flushPromises()
      expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('timeout')
      scanId = 'replacement-scan'
      useDevicesStore().applySnapshot({ identity: IDENTITY, devices: DEVICES, scanning: { scanId, found: 0, elapsedMs: 0, totalMs: 1000 } })
      await flushPromises()
      await wrapper.findAll('button').find(button => button.text() === messages.actions.retry)!.trigger('click')
      await flushPromises()
      expect(mocks.call.mock.calls.filter(([command]) => command === 'devices_scan_cancel')).toEqual([
        ['devices_scan_cancel', { scanId: 'original-scan' }]
      ])
      expect(useDevicesStore().scan?.scanId).toBe('replacement-scan')
      expect(wrapper.findComponent(ErrorNotice).exists()).toBe(false)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('discards a snapshot that returns after the page has been unmounted', async () => {
    let finish!: (value: unknown) => void
    mocks.call.mockImplementation(async (command: string) => command === 'devices_snapshot'
      ? new Promise((resolve) => { finish = resolve })
      : { ok: true, value: null })
    const wrapper = await mountDevices()
    await flushPromises()
    wrapper.unmount()
    finish({ ok: true, value: { identity: IDENTITY, devices: DEVICES } })
    await flushPromises()
    expect(useDevicesStore().cards).toEqual([])
  })
})
