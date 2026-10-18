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

vi.mock('../kernel', () => ({ kernel: { call: mocks.call, subscribe: mocks.subscribe } }))

const SettingsPage = (await import('./SettingsPage.vue')).default
const { IDENTITY } = await import('../ipc/fake-kernel-fixtures')
const { useSettingsStore } = await import('../stores/settings')
const ErrorNotice = (await import('../components/ErrorNotice.vue')).default

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

/** Answers by command name; anything unlisted is refused, so an unexpected call shows up as a failure. */
function answer(byCommand: Record<string, unknown>) {
  mocks.call.mockImplementation(async (command: string) =>
    command in byCommand
      ? { ok: true, value: byCommand[command] }
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
    mocks.call.mockImplementation(async (command: string) =>
      command === 'config_get'
        ? { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
        : { ok: true, value: IDENTITY }
    )

    const wrapper = mount(SettingsPage)
    await flushPromises()

    // ★ Through the component, not its words: the notice renders a translated sentence, so looking for the code in the
    // text asserts that the message table happens to include it. The prop is what the page owes.
    const notice = wrapper.findComponent(ErrorNotice)
    expect(notice.exists()).toBe(true)
    expect(notice.props('code')).toBe('internal')
    wrapper.unmount()
  })
})
