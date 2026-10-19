// @vitest-environment jsdom
/**
 * The settings page, mounted.
 *
 * # What is asserted
 *
 * That mounting reads the two things it needs, and that a refusal on either is shown rather than swallowed. The page has
 * no store — configuration is written back a field at a time, and there is no event that carries it — so the assertions
 * stop at what the kernel answered.
 *
 * # The three things this pattern needs, learned the hard way on the other pages
 *
 * `matchMedia`, `ResizeObserver`, `IntersectionObserver` — jsdom gives the first a **placeholder that is not a
 *   function**, so a guard like `if (!('matchMedia' in window))` installs nothing and hides the failure;
 * answers **by command name**, because a blanket `{ ok: true, value: [] }` fails far from the call that was wrong;
 * assertions on fields the DTO **actually has** — `toContain(undefined)` passes on `undefined`.
 */

import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  subscribe: vi.fn()
}))

const autostart = vi.hoisted(() => ({
  isEnabled: vi.fn(),
  enable: vi.fn(),
  disable: vi.fn()
}))

vi.mock('../kernel', () => ({ kernel: { call: mocks.call, subscribe: mocks.subscribe } }))
vi.mock('@tauri-apps/plugin-autostart', () => ({
  isEnabled: autostart.isEnabled,
  enable: autostart.enable,
  disable: autostart.disable
}))

const SettingsPage = (await import('./SettingsPage.vue')).default
const { IDENTITY } = await import('../ipc/fake-kernel-fixtures')
const { useSettingsStore } = await import('../stores/settings')
const ErrorNotice = (await import('../components/ErrorNotice.vue')).default
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

/** An answer for a recorder that has counted nothing, so a test about another section does not have to know about it. */
const NO_METRICS = { values: [], overflowedTotal: 0, maxSeriesPerMetric: 1_000 }

/**
 * Answers by command name; anything unlisted is refused, so an unexpected call shows up as a failure.
 *
 * ★ `diag_metrics` is seeded rather than left to be refused: the page reads it on mount by design, and a fixture that
 * refused it would put an error notice on every test here — including the ones asserting something else.
 */
function answer(byCommand: Record<string, unknown>) {
  const answers: Record<string, unknown> = { diag_metrics: NO_METRICS, ...byCommand }
  mocks.call.mockImplementation(async (command: string) =>
    command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
  )
}

beforeEach(() => {
  installBrowserApis()
  // The page reads `useSettingsStore`, so a fresh Pinia is part of the fixture: without it the store call throws before
  // any assertion runs.
  setActivePinia(createPinia())
  mocks.call.mockReset()
  mocks.subscribe.mockReset()
  mocks.subscribe.mockResolvedValue(() => {})
  autostart.isEnabled.mockReset()
  autostart.isEnabled.mockResolvedValue(false)
  autostart.enable.mockReset()
  autostart.enable.mockResolvedValue(undefined)
  autostart.disable.mockReset()
  autostart.disable.mockResolvedValue(undefined)
})

describe('the settings page', () => {
  it('reads the configuration and this device when it is mounted', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('config_get')
    expect(mocks.call).toHaveBeenCalledWith('identity_get')
    wrapper.unmount()
  })

  it('renames this device through the kernel and shows the name it read back', async () => {
    // ★ `identity_set_display_name` had a command, tests and no caller. A rename has to be followed by a read: the
    // answer is `Unit`, and the page's row and the sidebar are two different copies of the name.
    let reads = 0
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'diag_metrics')
        return { ok: true, value: NO_METRICS }
      if (command === 'config_get')
        return { ok: true, value: { values: {}, overridden: [] } }
      if (command === 'identity_get') {
        reads += 1
        return {
          ok: true,
          value: reads === 1 ? IDENTITY : { ...IDENTITY, displayName: 'Kitchen tablet', nameIsDefault: false }
        }
      }
      if (command === 'identity_set_display_name')
        return { ok: true, value: null }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    const rename = wrapper.findAll('button').find(button => button.text() === messages.settings.field.rename)
    expect(rename, 'the rename button is missing').toBeDefined()
    await rename?.trigger('click')

    const input = wrapper.find('input')
    expect(input.exists(), 'the name input did not open').toBe(true)
    await input.setValue('Kitchen tablet')

    const save = wrapper.findAll('button').find(button => button.text() === messages.settings.field.save)
    expect(save, 'the save button is missing').toBeDefined()
    await save?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('identity_set_display_name', { name: 'Kitchen tablet' })
    expect(wrapper.text()).toContain('Kitchen tablet')
    wrapper.unmount()
  })

  it('shows a preference the kernel has a value for', async () => {
    // ★ The bug this locks down: the page's own key names ('theme') are labels, and the kernel's are dotted ('ui.theme').
    // Reading the label returned `undefined`, so both controls showed "unset" no matter what was configured.
    answer({ config_get: { values: { 'ui.theme': 'dark' }, overridden: ['ui.theme'] }, identity_get: IDENTITY })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    expect(wrapper.text()).toContain('dark')
    wrapper.unmount()
  })

  it('writes the switch under the kernel key, and does not touch the store itself', async () => {
    // ★ Two claims in one: the patch names `ui.autoSyncPlugins` (the kernel's key, not the page's label), and the page does
    // **not** set the value locally — the kernel's `ConfigUpdated` event is what will change the store, so the switch must
    // still show the old value until that arrives.
    answer({ config_get: { values: { 'ui.autoSyncPlugins': false }, overridden: [] }, identity_get: IDENTITY })
    const wrapper = mount(SettingsPage)
    await flushPromises()

    await wrapper.findComponent({ name: 'ASwitch' }).vm.$emit('change', true)
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: { set: { 'ui.autoSyncPlugins': true } }
    })
    const store = useSettingsStore()
    expect(store.value('ui.autoSyncPlugins')).toBe(false)
    wrapper.unmount()
  })

  it('writes the cancel-confirmation preference under its own key', async () => {
    // ★ Absent means "ask": the page shows the switch on, and turning it off stores the one value that changes the
    // behaviour. A page that treated absent as off would remove the question without anybody asking it to.
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY, config_set: null })
    const wrapper = mount(SettingsPage)
    await flushPromises()

    const switches = wrapper.findAllComponents({ name: 'ASwitch' })
    expect(switches).toHaveLength(4)
    const cancelSwitch = switches[1]
    expect(cancelSwitch).toBeDefined()
    await cancelSwitch?.vm.$emit('change', false)
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: { set: { 'transfers.confirmCancel': false } }
    })
    wrapper.unmount()
  })

  it('writes the receive-notification switch under its own key', async () => {
    // ★ Absent means on: the design wants a file that arrives while the window is elsewhere to be noticed, so a page that
    // treated absent as off would silence it without anybody asking.
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY, config_set: null })
    const wrapper = mount(SettingsPage)
    await flushPromises()

    const notifySwitch = wrapper.findAllComponents({ name: 'ASwitch' })[2]
    expect(notifySwitch).toBeDefined()
    await notifySwitch?.vm.$emit('change', false)
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: { set: { 'transfers.notifyOnReceive': false } }
    })
    wrapper.unmount()
  })

  it('writes the registry URL, and unsets it when the field is cleared', async () => {
    // ★ Empty means "no registry", not "the empty string": the install branch treats an absent key as unsupported, and a
    // stored empty string would read as a configured address that is not one.
    answer({
      config_get: { values: { 'plugins.registry': 'https://r.example' }, overridden: ['plugins.registry'] },
      identity_get: IDENTITY,
      config_set: null
    })
    const wrapper = mount(SettingsPage)
    await flushPromises()

    const input = wrapper.find('input[placeholder="https://registry.example.com"]')
    expect((input.element as HTMLInputElement).value).toBe('https://r.example')

    await input.setValue('https://r2.example')
    const save = wrapper.findAll('button').find(button => button.text().includes('Save'))
    expect(save).toBeDefined()
    await save?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: { set: { 'plugins.registry': 'https://r2.example' } }
    })

    await input.setValue('')
    await save?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: { unset: ['plugins.registry'] }
    })
    wrapper.unmount()
  })

  it('writes one mirror as a string and several as an array', async () => {
    // ★ The kernel has accepted both shapes since mirrors landed; the page writes the string form for one URL so a
    // config written by an older settings page is not rewritten into a different shape just by opening this one.
    answer({
      config_get: {
        values: { 'plugins.registry': ['https://a.example', 'https://b.example'] },
        overridden: ['plugins.registry']
      },
      identity_get: IDENTITY,
      config_set: null
    })
    const wrapper = mount(SettingsPage)
    await flushPromises()

    expect(wrapper.findAll('input[placeholder="https://registry.example.com"]')).toHaveLength(2)

    const remove = wrapper.findAll('button').find(button => button.text().includes('Remove'))
    expect(remove).toBeDefined()
    await remove?.trigger('click')
    await flushPromises()
    const save = wrapper.findAll('button').find(button => button.text().includes('Save'))
    await save?.trigger('click')
    await flushPromises()
    const saves = mocks.call.mock.calls.filter(call => call[0] === 'config_set')
    expect(saves.at(-1)).toEqual([
      'config_set',
      { patch: { set: { 'plugins.registry': 'https://b.example' } } }
    ])

    const add = wrapper.findAll('button').find(button => button.text().includes('Add mirror'))
    await add?.trigger('click')
    await flushPromises()
    const inputs = wrapper.findAll('input[placeholder="https://registry.example.com"]')
    expect(inputs).toHaveLength(2)
    const second = inputs[1]
    expect(second).toBeDefined()
    await second?.setValue('https://c.example')
    await save?.trigger('click')
    await flushPromises()
    const after = mocks.call.mock.calls.filter(call => call[0] === 'config_set')
    expect(after.at(-1)).toEqual([
      'config_set',
      { patch: { set: { 'plugins.registry': ['https://b.example', 'https://c.example'] } } }
    ])
    wrapper.unmount()
  })

  it('shows this device\'s name, which is the one field the page can state without being told', async () => {
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    expect(wrapper.text()).toContain(IDENTITY.displayName)
    wrapper.unmount()
  })

  it('lists a trusted sender and takes the trust back by unsetting the last key', async () => {
    // ★ The list is the configuration key, not a second store: removing the only sender unsets the key rather than
    // writing an empty array, because "no preference" and "a preference for nobody" are the same thing to the kernel.
    const sender = '11111111-1111-4111-8111-111111111111'
    answer({
      config_get: { values: { 'transfers.acceptFrom': [sender] }, overridden: ['transfers.acceptFrom'] },
      identity_get: IDENTITY,
      config_set: null
    })
    const wrapper = mount(SettingsPage)
    await flushPromises()

    expect(wrapper.text()).toContain('Ask again')
    const remove = wrapper.findAll('button').find(button => button.text().includes('Ask again'))
    expect(remove, 'the trusted sender has a remove button').toBeDefined()
    await remove?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('config_set', { patch: { unset: ['transfers.acceptFrom'] } })
    wrapper.unmount()
  })

  it('shows the refusal when the configuration cannot be read', async () => {
    // Refused rather than answered: a settings screen that renders defaults when the read failed is showing the user
    // values the kernel never agreed to.
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'config_get')
        return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
      if (command === 'identity_get')
        return { ok: true, value: IDENTITY }
      if (command === 'diag_metrics')
        return { ok: true, value: NO_METRICS }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    // ★ Through the component, not its words: the notice renders a translated sentence, so looking for the code in the
    // text asserts that the message table happens to include it. The prop is what the page owes.
    const notice = wrapper.findComponent(ErrorNotice)
    expect(notice.exists()).toBe(true)
    expect(notice.props('code')).toBe('internal')
    wrapper.unmount()
  })

  it('draws the metrics the recorder holds, keyed the way the kernel names them', async () => {
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_metrics: {
        values: [
          {
            key: { name: 'host_calls_total', labels: [['plugin', '@acme/hello']] },
            value: { kind: 'counter', value: 7 }
          },
          {
            key: { name: 'host_call_ms', labels: [] },
            value: { kind: 'histogram', value: { bounds: [1, 10], buckets: [2, 3, 1], count: 6, sum: 30 } }
          }
        ],
        overflowedTotal: 0,
        maxSeriesPerMetric: 1_000
      }
    })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    // ★ The rendered key is the kernel's own spelling (`name{label=value}`), not a page-invented one: a log line, a
    // diagnostic export and this panel have to name the same series.
    expect(wrapper.text()).toContain('host_calls_total{plugin=@acme/hello}')
    expect(wrapper.text()).toContain('7')
    expect(wrapper.text()).toContain(messages.settings.metrics.histogram(6, '5.00'))
    wrapper.unmount()
  })

  it('compares the whole start against the cold-start budget', async () => {
    // ★ The panel already lists every series; this is the one reading whose value is a **threshold**. A number nobody
    // compares is not a budget, and an unobserved series must not read as "0 ms and within budget".
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_metrics: {
        values: [
          {
            key: { name: 'startup_duration_ms', labels: [['phase', 'state']] },
            value: { kind: 'histogram', value: { bounds: [1], buckets: [1, 0], count: 1, sum: 40 } }
          },
          {
            key: { name: 'startup_duration_ms', labels: [['phase', 'total']] },
            value: { kind: 'histogram', value: { bounds: [1, 10, 100, 1000], buckets: [0, 0, 0, 2, 0], count: 2, sum: 700 } }
          }
        ],
        overflowedTotal: 0,
        maxSeriesPerMetric: 1_000
      }
    })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    expect(wrapper.text()).toContain(messages.settings.metrics.startup)
    expect(wrapper.text()).toContain('350 ms')
    expect(wrapper.text()).toContain(messages.settings.metrics.startupWithin)
    wrapper.unmount()
  })

  it('asks the platform to start at login', async () => {
    // ★ The registration belongs to the platform. The page reads `isEnabled` on mount and calls `enable`/`disable`; it
    // does not keep a preference of its own, because a stored copy can disagree with what the OS actually does.
    answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY, config_set: null })
    const wrapper = mount(SettingsPage)
    await flushPromises()

    const launch = wrapper.findAllComponents({ name: 'ASwitch' })[3]
    expect(launch).toBeDefined()
    await launch?.vm.$emit('change', true)
    await flushPromises()

    expect(autostart.enable).toHaveBeenCalled()
    wrapper.unmount()
  })

  it('says when the snapshot is incomplete rather than drawing it as the whole', async () => {
    // ★ The cap is a real answer about the data ("some series were folded"), not a warning to suppress: a panel that drew
    // twenty rows and stayed silent would be a panel claiming the recorder holds twenty things.
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_metrics: {
        values: [{ key: { name: 'host_calls_total', labels: [] }, value: { kind: 'counter', value: 1 } }],
        overflowedTotal: 3,
        maxSeriesPerMetric: 2
      }
    })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    expect(wrapper.text()).toContain(messages.settings.metrics.overflow(3, 2))
    wrapper.unmount()
  })

  it('shows the audit retention and writes a new window', async () => {
    // ★ The setting existed only as a constant in the kernel. It is a configuration key now, and the screen is where it is
    // edited — a key no screen writes is a setting the user cannot find.
    answer({
      config_get: { values: { 'audit.retentionDays': 30 }, overridden: ['audit.retentionDays'] },
      identity_get: IDENTITY,
      config_set: null
    })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    const number = wrapper.findComponent({ name: 'AInputNumber' })
    expect(number.exists(), 'the retention control is missing').toBe(true)
    expect(number.props('value'), 'the configured window reaches the box').toBe(30)

    await number.vm.$emit('update:value', 90)
    await flushPromises()

    // ★ The button next to the box, rather than "the first Save on the page": the registry section has one too, and an
    // assertion that clicked the wrong one would pass while the setting was never written.
    const button = number.element.parentElement?.querySelector('button')
    expect(button, 'the box drew no way to save it').toBeDefined()
    button?.click()
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('config_set', {
      patch: { set: { 'audit.retentionDays': 90 } }
    })
    wrapper.unmount()
  })

  it('exports a diagnostic bundle and shows what it holds', async () => {
    // ★ The bundle is the one result a user has to read back: a path alone says nothing about whether the audit trail
    // went in, and the two optional sections are exactly the ones that name things they might not mean to share.
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_export: {
        path: 'C:/data/synra-diagnostics.json',
        included: ['environment', 'config'],
        excluded: ['audit trail: not requested']
      }
    })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    const boxes = wrapper.findAllComponents({ name: 'ACheckbox' })
    expect(boxes.length, 'the two optional sections are not drawn').toBeGreaterThanOrEqual(2)
    await boxes[0]?.vm.$emit('update:checked', true)

    const button = wrapper.findAll('button').find(candidate => candidate.text() === messages.settings.diagnostics.export)
    expect(button, 'the export button is missing').toBeDefined()
    await button?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('diag_export', {
      options: { includeAudit: true, includeDebugLogs: false }
    })
    expect(wrapper.text()).toContain('C:/data/synra-diagnostics.json')
    expect(wrapper.text()).toContain(messages.settings.diagnostics.included('environment, config'))
    expect(wrapper.text()).toContain(messages.settings.diagnostics.excluded('audit trail: not requested'))
    wrapper.unmount()
  })

  it('dismisses the crash reports that would start the next run in safe mode', async () => {
    // ★ Safe mode is decided from the reports on disk, so "ignore" has to delete them. A page that only hid the prompt
    // would ask again next start — which is exactly how a person learns to click through it without reading.
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_crash_dismiss: null
    })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    const button = wrapper
      .findAll('button')
      .find(candidate => candidate.text() === messages.settings.diagnostics.dismissCrashes)
    expect(button, 'the dismiss button is missing').toBeDefined()
    await button?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('diag_crash_dismiss')
    expect(wrapper.text()).toContain(messages.settings.diagnostics.dismissed)
    wrapper.unmount()
  })

  it('shows the crash reports on request, newest first as the kernel answers them', async () => {
    // ★ "查看详情" reads on request. A page that read them on mount would make every visit to settings touch a directory
    // the user did not ask about, and the reports are the one diagnostic that can contain a panic message.
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_crashes: [
        {
          atMs: 2_000,
          version: '0.1.0',
          location: 'src/plugin.rs:42',
          message: 'the plugin panicked'
        }
      ]
    })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    const button = wrapper
      .findAll('button')
      .find(candidate => candidate.text() === messages.settings.diagnostics.showCrashes)
    expect(button, 'the show button is missing').toBeDefined()
    await button?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('diag_crashes')
    expect(wrapper.text()).toContain('the plugin panicked')
    expect(wrapper.text()).toContain('src/plugin.rs:42')
    wrapper.unmount()
  })

  it('reads the recent log when its section is opened', async () => {
    // ★ Read on open, not on mount: a log copied at load is stale by the time a user scrolls to it, and it is the
    // largest thing this page can ask for. The queued `toggle` is the browser's own event, as in the audit section.
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_logs_tail: ['synra: started', 'synra: discovery bound']
    })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    expect(mocks.call).not.toHaveBeenCalledWith('diag_logs_tail', expect.anything())
    const log = wrapper.find('details')
    expect(log.exists(), 'the log section is missing').toBe(true)
    ;(log.element as HTMLDetailsElement).open = true
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('diag_logs_tail', { lines: 200 })
    expect(wrapper.text()).toContain('synra: discovery bound')
    wrapper.unmount()
  })

  it('runs the startup probes on request and shows each result', async () => {
    // ★ On request, not on mount: the probes bind ports and touch the disk, and a settings page that ran them on every
    // visit would be doing work the user did not ask for. The assertion above the click is what holds that.
    answer({
      config_get: { values: {}, overridden: [] },
      identity_get: IDENTITY,
      diag_probes: {
        results: [
          { name: 'tcp_port_available', status: 'ok', detail: 'a free port was found' },
          { name: 'wasm_backend', status: 'warn', detail: 'this host did not read the runtime', suggestion: 'include this report' }
        ]
      }
    })

    const wrapper = mount(SettingsPage)
    await flushPromises()

    expect(mocks.call).not.toHaveBeenCalledWith('diag_probes')
    const button = wrapper.findAll('button').find(candidate => candidate.text() === messages.settings.diagnostics.probes)
    expect(button, 'the probe button is missing').toBeDefined()
    await button?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('diag_probes')
    expect(wrapper.text()).toContain('tcp_port_available')
    expect(wrapper.text()).toContain(messages.settings.diagnostics.probeStatus('warn'))
    expect(wrapper.text()).toContain('include this report')

    // ★ The summary is counts, not a colour: a report of twelve rows is one sentence until something fails. Then the
    // filter hides exactly the passes, which is the difference between "nothing to do" and "nothing worth reading".
    expect(wrapper.text()).toContain(messages.settings.diagnostics.probesSummary(1, 1, 0))
    const filter = wrapper.findAllComponents({ name: 'ACheckbox' }).at(-1)
    expect(filter, 'the only-problems filter is missing').toBeDefined()
    await filter?.vm.$emit('update:checked', true)
    await flushPromises()
    expect(wrapper.text()).not.toContain('tcp_port_available')
    expect(wrapper.text()).toContain('wasm_backend')
    wrapper.unmount()
  })
})
