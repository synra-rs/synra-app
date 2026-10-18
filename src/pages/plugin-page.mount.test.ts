// @vitest-environment jsdom
/**
 * The plugins page, mounted — the one component test in this repository.
 *
 * # What makes this possible, after one failed attempt
 *
 * The first try stubbed `ant-design-vue` out of the way and everything past the second mount broke: an `AList` draws its
 * rows through the `renderItem` **scoped slot**, which a stub that only renders `slots.default` never calls. So the real
 * components stay, and what this file supplies instead is the three browser APIs jsdom does not implement and antd
 * reaches for: `matchMedia`, `ResizeObserver` and `IntersectionObserver`.
 *
 * # What is asserted, and what is not
 *
 * Only what the page itself is responsible for: that mounting reads the list, that the card's words reach the DOM, that a
 * refusal is shown rather than swallowed, and that a card put in the store by **something other than the page's own read**
 * reaches the screen. Clicking through antd's confirm dialog is not tested — that is antd's behaviour, and the arguments
 * those clicks produce are asserted in `pages/plugins/actions.test.ts`.
 */

import type { PluginSnapshot } from '@synra/protocol'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  subscribe: vi.fn()
}))

vi.mock('../kernel', () => ({ kernel: { call: mocks.call, subscribe: mocks.subscribe } }))

const PluginsPage = (await import('./PluginsPage.vue')).default
const { usePluginsStore } = await import('../stores/plugins')
const ErrorNotice = (await import('../components/ErrorNotice.vue')).default
const { messages } = await import('../messages')

/**
 * What jsdom lacks and antd asks for.
 *
 * ★ Assigned on **every** test rather than once. jsdom gives `window` a `matchMedia` that is not a function (it exists
 * only as a placeholder), and the first attempt guarded on `'matchMedia' in window` — which was true, so nothing was
 * installed, the first mounts still worked, and the third test died with "window.matchMedia is not a function". A guard
 * that asks the wrong question is worse than no guard: it made the first two tests pass and hid the reason.
 */
function installBrowserApis(): void {
  window.matchMedia = (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false
  }) as MediaQueryList

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

installBrowserApis()

/** A card shaped like the kernel's, with only the fields the page reads. */
function card(overrides: Partial<PluginSnapshot> = {}): PluginSnapshot {
  return {
    pluginId: '@acme/hello',
    title: 'Hello',
    description: null,
    version: '1.0.0',
    publisher: 'acme',
    enabled: false,
    state: 'awaiting_consent',
    hybrid: false,
    hasUi: false,
    health: undefined,
    granted: [],
    denied: [],
    requested: ['log:*'],
    update: undefined,
    ...overrides
  } as PluginSnapshot
}

beforeEach(() => {
  // A fresh pinia per test: the store is what the page reads, so a card left by the previous test would be the page
  // showing something this test never asked for.
  installBrowserApis()
  setActivePinia(createPinia())
  mocks.call.mockReset()
  mocks.subscribe.mockReset()
  mocks.subscribe.mockResolvedValue(() => {})
})

describe('the plugins page', () => {
  it('asks for the list when it is mounted', async () => {
    mocks.call.mockResolvedValue({ ok: true, value: [] })

    const wrapper = mount(PluginsPage)
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('plugins_list')
    wrapper.unmount()
  })

  it('renders what the kernel answered', async () => {
    mocks.call.mockResolvedValue({ ok: true, value: [card()] })

    const wrapper = mount(PluginsPage)
    await flushPromises()

    expect(wrapper.text()).toContain('Hello')
    expect(wrapper.text()).toContain('acme')
    wrapper.unmount()
  })

  it('shows what the plugin asks for, not only what it was given', async () => {
    // The consent checkboxes are built from `requested`; a page that rendered `granted` would show an empty prompt for a
    // plugin nobody had decided about. `granted` is deliberately empty here, so the assertion can only pass through
    // `requested`.
    mocks.call.mockResolvedValue({ ok: true, value: [card({ granted: [], denied: [] })] })

    const wrapper = mount(PluginsPage)
    await flushPromises()

    expect(wrapper.text()).toContain('log:*')
    wrapper.unmount()
  })

  it('shows the refusal when the list cannot be read', async () => {
    mocks.call.mockResolvedValue({ ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } })

    const wrapper = mount(PluginsPage)
    await flushPromises()

    // ★ Asserted through the component rather than its words: the notice renders a translated sentence, so looking for the
    // code in the text would be asserting that the message table happens to include it. The prop is what the page owes.
    const notice = wrapper.findComponent(ErrorNotice)
    expect(notice.exists()).toBe(true)
    expect(notice.props('code')).toBe('internal')
    wrapper.unmount()
  })

  it('opens a page from the card that has one', async () => {
    // ★ The command had no caller until this button: `plugin_ui_open` was registered, tested and unreachable. What this
    // asserts is the join — the card's `hasUi` draws the control, and pressing it reaches the command with the id.
    mocks.call.mockResolvedValue({ ok: true, value: [card({ hasUi: true })] })

    const wrapper = mount(PluginsPage)
    await flushPromises()

    const button = wrapper.findAll('button').find(candidate => candidate.text() === messages.plugins.openAction)
    expect(button, 'the card with a page drew no way to open it').toBeDefined()
    await button?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('plugin_ui_open', { pluginId: '@acme/hello' })
    wrapper.unmount()
  })

  it('draws no open button for a plugin without a page', async () => {
    // A button that can only refuse is worse than no button: the card already has the word for what the plugin is.
    mocks.call.mockResolvedValue({ ok: true, value: [card({ hasUi: false })] })

    const wrapper = mount(PluginsPage)
    await flushPromises()

    expect(wrapper.findAll('button').some(candidate => candidate.text() === messages.plugins.openAction)).toBe(false)
    wrapper.unmount()
  })

  it('plans a sync and sends only when a device needs the artifact', async () => {
    // ★ The plan is read before the run: `online` exists only in the plan, and a page that ran blind would report a
    // failure for every device whose network was already visibly down.
    const plan = {
      pluginId: '@acme/hello',
      targets: [{ deviceId: '11111111-1111-4111-8111-111111111111', displayName: 'Laptop', needsArtifact: true, online: true }]
    }
    mocks.call
      .mockResolvedValueOnce({ ok: true, value: [card()] })
      .mockResolvedValueOnce({ ok: true, value: [] })
      .mockResolvedValueOnce({ ok: true, value: plan })
      .mockResolvedValueOnce({ ok: true, value: { updated: 1, unchanged: 0 } })

    const wrapper = mount(PluginsPage)
    await flushPromises()

    const button = wrapper.findAll('button').find(candidate => candidate.text() === messages.plugins.syncAction)
    expect(button, 'the card drew no sync button').toBeDefined()
    await button?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenNthCalledWith(3, 'plugins_sync_plan', { pluginId: '@acme/hello' })
    expect(mocks.call).toHaveBeenNthCalledWith(4, 'plugins_sync_run', { plan })
    expect(wrapper.text()).toContain(messages.plugins.syncDone(1, 0, 0))
    wrapper.unmount()
  })

  it('says there is nothing to send when every device already has it', async () => {
    mocks.call
      .mockResolvedValueOnce({ ok: true, value: [card()] })
      .mockResolvedValueOnce({ ok: true, value: [] })
      .mockResolvedValueOnce({
        ok: true,
        value: {
          pluginId: '@acme/hello',
          targets: [{ deviceId: '11111111-1111-4111-8111-111111111111', displayName: 'Laptop', needsArtifact: false, online: true }]
        }
      })

    const wrapper = mount(PluginsPage)
    await flushPromises()

    await wrapper.findAll('button').find(candidate => candidate.text() === messages.plugins.syncAction)?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledTimes(3)
    expect(wrapper.text()).toContain(messages.plugins.syncNothing)
    wrapper.unmount()
  })

  it('installs from the registry when the mode says so', async () => {
    // ★ The mode is a control rather than a guess about the text: `@acme/hello` and `C:\plugins\hello` are both "a string
    // with a slash", and the source the command receives is what decides whether a network is touched.
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list')
        return { ok: true, value: [card()] }
      if (command === 'plugins_publishers')
        return { ok: true, value: [] }
      if (command === 'plugins_preview') {
        return {
          ok: true,
          value: {
            pluginId: '@acme/hello',
            version: '1.0.0',
            title: 'Hello',
            publisher: '@acme',
            capabilities: ['log:*'],
            headless: true,
            hasUi: false
          }
        }
      }
      if (command === 'plugins_install')
        return { ok: true, value: { pluginId: '@acme/hello', version: '1.0.0', enabled: false } }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })

    const wrapper = mount(PluginsPage)
    await flushPromises()

    const mode = wrapper.findComponent({ name: 'ASegmented' })
    await mode.vm.$emit('update:value', 'registry')
    await flushPromises()
    await wrapper.find('input[placeholder="Plugin id, e.g. @acme/hello"]').setValue('@acme/hello')
    const install = wrapper.findAll('button').find(button => button.text().includes('Install'))
    await install?.trigger('click')
    await flushPromises()

    // ★ The registry install asks first: the click fetches the package's own description, and nothing is downloaded until
    // the dialog is confirmed.
    expect(mocks.call).toHaveBeenCalledWith('plugins_preview', { pluginId: '@acme/hello' })
    expect(mocks.call).not.toHaveBeenCalledWith('plugins_install', expect.anything())

    const confirm = Array.from(document.body.querySelectorAll('button')).find(button =>
      button.textContent?.includes('Install')
    )
    expect(confirm).toBeDefined()
    confirm?.click()
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('plugins_install', {
      source: { source: 'registry', pluginId: '@acme/hello' }
    })
    wrapper.unmount()
  })

  it('loads the next page with the offset it already has', async () => {
    // ★ A search without a total gives only one signal that more may exist: the last page was full. The offset is the
    // number already shown, so a result the registry repeats is deduplicated rather than appended twice.
    const page = (count: number, start: number) =>
      Array.from({ length: count }, (_, index) => ({
        pluginId: `@acme/p${start + index}`,
        title: `P${start + index}`,
        version: '1.0.0',
        publisher: 'acme',
        capabilityCount: 0,
        installed: false
      }))
    mocks.call.mockImplementation(async (command: string, args?: { query?: { offset?: number } }) => {
      if (command === 'plugins_list')
        return { ok: true, value: [] }
      if (command === 'plugins_publishers')
        return { ok: true, value: [] }
      if (command === 'plugins_catalog_search') {
        const offset = args?.query?.offset ?? 0
        return { ok: true, value: offset === 0 ? page(20, 0) : page(1, 20) }
      }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })

    const wrapper = mount(PluginsPage)
    await flushPromises()

    await wrapper.find('input[placeholder="Search the registry"]').setValue('p')
    const search = wrapper.findAll('button').find(button => button.text().includes('Search'))
    await search?.trigger('click')
    await flushPromises()

    const more = wrapper.findAll('button').find(button => button.text().includes('Load more'))
    expect(more).toBeDefined()
    await more?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('plugins_catalog_search', {
      query: { query: 'p', offset: 20, sort: 'relevance' }
    })
    expect(wrapper.text()).toContain('P20')
    wrapper.unmount()
  })

  it('filters the installed list by what a person remembers', async () => {
    // ★ A filter over the installed list, not a registry search: this shell has no registry client, and a box that
    // promised the catalogue would be a box that cannot deliver. Matched on title here; the id and description are the
    // other two arms of the same predicate.
    mocks.call.mockResolvedValue({
      ok: true,
      value: [card(), card({ pluginId: '@acme/other', title: 'Other' })]
    })

    const wrapper = mount(PluginsPage)
    await flushPromises()

    await wrapper.find('input[placeholder="Filter installed plugins"]').setValue('other')
    expect(wrapper.text()).toContain('Other')
    expect(wrapper.text()).not.toContain('Hello')
    wrapper.unmount()
  })

  it('draws the store, so a card that arrived by event reaches the screen', async () => {
    // ★ The page stopped holding a copy of the list: it reads the store, and `src/events.ts` writes it. This asserts the
    // join — a card put in the store by something other than the page's own read still reaches the DOM.
    mocks.call.mockResolvedValue({ ok: true, value: [] })
    const wrapper = mount(PluginsPage)
    await flushPromises()

    usePluginsStore().upsert(card({ title: 'Arrived by event' }))
    await flushPromises()

    expect(wrapper.text()).toContain('Arrived by event')
    wrapper.unmount()
  })
})
