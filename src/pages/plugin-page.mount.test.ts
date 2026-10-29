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
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter, createWebHashHistory } from 'vue-router'
import { PLUGINS } from '../ipc/fake-kernel-fixtures'
import { setLocalePreference } from '../locale'
import { formatDateTime } from '../locale-format'

const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  subscribe: vi.fn(),
  open: vi.fn()
}))

vi.mock('../kernel', () => ({ kernel: { call: mocks.call, subscribe: mocks.subscribe } }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }))

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
    dataPath: '/tmp/synra/plugin-data/@acme-hello',
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

let pageRouter: ReturnType<typeof createRouter>

function mountPlugins(options: { attachTo?: HTMLElement } = {}) {
  return mount(PluginsPage, { ...options, global: { plugins: [pageRouter] } })
}

beforeEach(async () => {
  pageRouter = createRouter({ history: createMemoryHistory(), routes: [{ path: '/plugins', component: PluginsPage }] })
  await pageRouter.push('/plugins')
  await pageRouter.isReady()
  // A fresh pinia per test: the store is what the page reads, so a card left by the previous test would be the page
  // showing something this test never asked for.
  installBrowserApis()
  setActivePinia(createPinia())
  mocks.call.mockReset()
  mocks.subscribe.mockReset()
  mocks.subscribe.mockResolvedValue(() => {})
  mocks.open.mockReset()
})

afterEach(() => {
  setLocalePreference('system')
  window.history.replaceState({}, '', '/')
})

describe('the plugins page', () => {
  it('opens installation on demand and preserves its draft without background reads', async () => {
    const answers: Record<string, unknown> = { plugins_list: [card({ state: 'active' })], plugins_publishers: [] }
    mocks.call.mockImplementation(async (command: string) => command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } })
    const wrapper = mountPlugins({ attachTo: document.body })
    try {
      await flushPromises()
      const install = wrapper.get('#plugin-install')
      expect(install.isVisible()).toBe(false)
      await wrapper.get('[data-testid="install-toggle"]').trigger('click')
      await flushPromises()
      expect(install.isVisible()).toBe(true)
      await install.get('input[aria-label]').setValue('C:/plugins/starter/dist')
      await flushPromises()
      await wrapper.get('[data-testid="install-toggle"]').trigger('click')
      await flushPromises()
      await wrapper.get('[data-testid="install-toggle"]').trigger('click')
      await flushPromises()
      expect((install.get('input[aria-label]').element as HTMLInputElement).value).toBe('C:/plugins/starter/dist')
      const management = wrapper.get('[data-plugin-management] .ant-collapse-header')
      expect(management.attributes('aria-expanded')).toBe('false')
      await management.trigger('click')
      await flushPromises()
      expect(management.attributes('aria-expanded')).toBe('true')
      expect(mocks.call.mock.calls.map(([command]) => command)).toEqual(['plugins_list', 'plugins_publishers'])
    }
    finally {
      wrapper.unmount()
    }
  })

  it('preserves a refused install draft and restores focus after a successful retry', async () => {
    const answers: Record<string, unknown> = { plugins_list: [], plugins_publishers: [] }
    let refused = true
    mocks.call.mockImplementation(async (command: string) => command in answers
      ? { ok: true, value: answers[command] }
      : command === 'plugins_install' && !refused
        ? { ok: true, value: { pluginId: '@acme/hello', version: '1.0.0', enabled: false } }
        : { ok: false, error: { code: 'invalid', message: 'error.invalid', retryable: false } })
    const wrapper = mountPlugins({ attachTo: document.body })
    try {
      await flushPromises()
      await wrapper.get('[data-testid="install-toggle"]').trigger('click')
      await flushPromises()
      await wrapper.get('#plugin-install input[aria-label]').setValue('C:/incomplete-plugin')
      await flushPromises()
      await wrapper.get('[data-testid="install-submit"]').trigger('click')
      await flushPromises()
      expect(mocks.call).toHaveBeenCalledWith('plugins_install', { source: { source: 'path', path: 'C:/incomplete-plugin' } })
      expect(wrapper.get('#plugin-install').isVisible()).toBe(true)
      expect((wrapper.get('#plugin-install input[aria-label]').element as HTMLInputElement).value).toBe('C:/incomplete-plugin')
      expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('invalid')
      refused = false
      await wrapper.get('[data-testid="install-submit"]').trigger('click')
      await flushPromises()
      expect(wrapper.get('#plugin-install').isVisible()).toBe(false)
      expect(document.activeElement).toBe(wrapper.get('[data-testid="install-toggle"]').element)
    }
    finally {
      wrapper.unmount()
    }
  })

  it.each(['en', 'zh-CN'])('shows only actionable preview health notices in %s', async (language) => {
    setLocalePreference(language)
    const answers: Record<string, unknown> = { plugins_list: PLUGINS, plugins_publishers: [] }
    mocks.call.mockImplementation(async (command: string) => command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } })
    const wrapper = mountPlugins()
    try {
      await flushPromises()
      const notices = wrapper.findAllComponents({ name: 'AAlert' })
      expect(notices.some(notice => notice.props('message') === messages.plugins.healthLabel('good'))).toBe(false)
      const fault = notices.find(notice => notice.props('message') === messages.plugins.healthLabel('faulted'))
      expect(fault?.props('type')).toBe('error')
      expect(fault?.props('description')).toBeUndefined()
      expect(wrapper.text()).not.toContain('plugin.health.')
    }
    finally {
      wrapper.unmount()
    }
  })

  it('keeps the install input named as its source and language change', async () => {
    setLocalePreference('en')
    const answers: Record<string, unknown> = { plugins_list: [], plugins_publishers: [] }
    mocks.call.mockImplementation(async (command: string) => command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } })
    const wrapper = mountPlugins()
    try {
      await flushPromises()
      expect(wrapper.get('input[aria-label]').attributes('aria-label')).toBe(messages.plugins.installPath)
      await wrapper.findComponent({ name: 'ASegmented' }).vm.$emit('update:value', 'registry')
      await flushPromises()
      const input = wrapper.get('input[aria-label]')
      expect(input.attributes('aria-label')).toBe(messages.plugins.installName)
      await input.setValue('@acme/hello')
      setLocalePreference('zh-CN')
      await flushPromises()
      expect(wrapper.get('input[aria-label]').attributes('aria-label')).toBe(messages.plugins.installName)
      expect((wrapper.get('input[aria-label]').element as HTMLInputElement).value).toBe('@acme/hello')
    }
    finally {
      wrapper.unmount()
    }
  })

  it('fills the install path from the platform directory picker', async () => {
    // ★ The path was a hand-typed string. The picker answers the same string the input takes, so the install path itself
    // is unchanged — only the way it is produced is.
    mocks.open.mockResolvedValue('C:/plugins/hello')
    mocks.call.mockResolvedValue({ ok: true, value: [] })

    const wrapper = mountPlugins()
    await flushPromises()

    const choose = wrapper.findAll('button').find(button => button.text() === messages.plugins.chooseDirectory)
    expect(choose, 'the choose-folder button is missing').toBeDefined()
    await choose?.trigger('click')
    await flushPromises()

    expect(mocks.open).toHaveBeenCalledWith(expect.objectContaining({ directory: true, multiple: false }))
    const field = wrapper.findAll('input').find(input => (input.element as HTMLInputElement).type === 'text')
    expect(field, 'the install input is missing').toBeDefined()
    expect((field!.element as HTMLInputElement).value).toBe('C:/plugins/hello')
    wrapper.unmount()
  })

  it('fills the install path from a chosen .synra package', async () => {
    // ★ The same input takes a directory or a package, and the pipeline branches on what it received. This picker only
    // chooses which dialog opens; the filter is a hint, not the rule.
    mocks.open.mockResolvedValue('C:/plugins/hello.synra')
    mocks.call.mockResolvedValue({ ok: true, value: [] })

    const wrapper = mountPlugins()
    await flushPromises()

    const choose = wrapper.findAll('button').find(button => button.text() === messages.plugins.choosePackage)
    expect(choose, 'the choose-package button is missing').toBeDefined()
    await choose?.trigger('click')
    await flushPromises()

    expect(mocks.open).toHaveBeenCalledWith(expect.objectContaining({
      directory: false,
      multiple: false,
      filters: [{ name: 'Synra package', extensions: ['synra'] }]
    }))
    const field = wrapper.findAll('input').find(input => (input.element as HTMLInputElement).type === 'text')
    expect(field, 'the install input is missing').toBeDefined()
    expect((field!.element as HTMLInputElement).value).toBe('C:/plugins/hello.synra')
    wrapper.unmount()
  })

  it('opens the exact plugin from a hash route and follows query-only navigation', async () => {
    pageRouter.options.history.destroy()
    window.history.replaceState({}, '', '/#/plugins?plugin=%40acme%2Fhello')
    pageRouter = createRouter({ history: createWebHashHistory(), routes: [{ path: '/plugins', component: PluginsPage }] })
    mocks.call.mockImplementation(async (command: string) => command === 'plugins_list'
      ? { ok: true, value: [card(), card({ pluginId: '@acme/hello-extra', title: 'Other plugin' })] }
      : { ok: true, value: [] })

    const wrapper = mountPlugins({ attachTo: document.body })
    try {
      await pageRouter.isReady()
      await flushPromises()
      expect(wrapper.get('[data-testid="plugin-target"]').text()).toContain('Hello')
      expect(wrapper.text()).not.toContain('Other plugin')
      expect(wrapper.get('[data-plugin-management] .ant-collapse-header').attributes('aria-expanded')).toBe('true')
      expect(document.activeElement).toBe(wrapper.get('[data-testid="plugin-target"] span').element)
      await pageRouter.push({ path: '/plugins', query: { plugin: '@acme/hello-extra', origin: 'notice' } })
      await flushPromises()
      expect(wrapper.get('[data-testid="plugin-target"]').text()).toContain('Other plugin')
      expect(wrapper.get('[data-plugin-management] .ant-collapse-header').attributes('aria-expanded')).toBe('true')
      await wrapper.get('[data-testid="plugin-target"] button').trigger('click')
      await flushPromises()
      expect(pageRouter.currentRoute.value.query).toEqual({ origin: 'notice' })
      expect(window.location.hash).toBe('#/plugins?origin=notice')
      expect(document.activeElement).toBe(wrapper.get(`input[placeholder="${messages.plugins.filterPlaceholder}"]`).element)
      expect(wrapper.text()).toContain('Hello')
      expect(wrapper.text()).toContain('Other plugin')
      expect(mocks.call.mock.calls.map(([command]) => command)).toEqual(['plugins_list', 'plugins_publishers'])
    }
    finally {
      wrapper.unmount()
      pageRouter.options.history.destroy()
    }
  })

  it('explains a missing notification target and returns to the installed list', async () => {
    await pageRouter.push({ path: '/plugins', query: { plugin: '@acme/missing' } })
    mocks.call.mockImplementation(async (command: string) => command === 'plugins_list'
      ? { ok: true, value: [card()] }
      : { ok: true, value: [] })

    const wrapper = mountPlugins()
    await flushPromises()

    expect(wrapper.text()).toContain(messages.plugins.notFound('@acme/missing'))
    const clear = wrapper.findAll('button').find(button => button.text() === messages.plugins.showAll)
    expect(clear, 'the empty list has no clear action').toBeDefined()
    await clear?.trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Hello')
    expect(pageRouter.currentRoute.value.fullPath).toBe('/plugins')
    wrapper.unmount()
  })

  it('keeps local filter drafts separate from notification targets and permission review', async () => {
    mocks.call.mockImplementation(async (command: string) => command === 'plugins_list'
      ? { ok: true, value: [card(), card({ pluginId: '@acme/other', title: 'Other plugin', state: 'active', granted: ['log:*'] })] }
      : { ok: true, value: [] })
    const wrapper = mountPlugins()
    try {
      await flushPromises()
      expect(wrapper.findAll('button').filter(button => button.text() === messages.plugins.reviewPermissions)).toHaveLength(1)
      expect(wrapper.text()).not.toContain('awaiting_consent')
      await wrapper.get(`input[placeholder="${messages.plugins.filterPlaceholder}"]`).setValue('Other plugin')
      await pageRouter.push({ path: '/plugins', query: { plugin: '@acme/hello' } })
      await flushPromises()
      expect(wrapper.get('[data-testid="plugin-target"]').text()).toContain('Hello')
      await wrapper.get('[data-testid="plugin-target"] button').trigger('click')
      await flushPromises()
      expect((wrapper.get(`input[placeholder="${messages.plugins.filterPlaceholder}"]`).element as HTMLInputElement).value).toBe('Other plugin')
      expect(wrapper.text()).not.toContain(messages.plugins.reviewPermissions)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('asks for the list when it is mounted', async () => {
    mocks.call.mockResolvedValue({ ok: true, value: [] })

    const wrapper = mountPlugins()
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('plugins_list')
    wrapper.unmount()
  })

  it('renders what the kernel answered', async () => {
    mocks.call.mockResolvedValue({ ok: true, value: [card()] })

    const wrapper = mountPlugins()
    await flushPromises()

    expect(wrapper.text()).toContain('Hello')
    expect(wrapper.text()).toContain('acme')
    wrapper.unmount()
  })

  it('colours an active plugin as success, which is the word the kernel now returns', async () => {
    // ★ The kernel used to answer `running` for a loaded plugin while this page's colour table and fixtures used `active`.
    // The green tag existed and was unreachable, so a running plugin looked as neutral as one with no state at all.
    mocks.call.mockResolvedValue({ ok: true, value: [card({ state: 'active', enabled: true })] })

    const wrapper = mountPlugins()
    await flushPromises()

    expect(wrapper.find('.ant-tag-success').exists(), 'an active plugin is not shown as success').toBe(true)
    wrapper.unmount()
  })

  it('warns when a loaded plugin is over its limits', async () => {
    // ★ The health rung, which the shell used to leave as `None` for every card. A degraded plugin is still `enabled` and
    // may still say `running`, so the warning explains the reported rung in the selected interface language.
    mocks.call.mockResolvedValue({
      ok: true,
      value: [card({
        health: {
          health: 'degraded',
          explanation: 'this plugin is over its limits, so it is running at a reduced quota',
          callsAdmitted: 10,
          callsRefused: 3,
          usage: {
            maxMemoryBytes: 64 * 1024 * 1024,
            maxInstances: 4,
            maxStorageBytes: 128 * 1024 * 1024,
            maxOpenTransfers: 3,
            hostCallsPerSec: 20,
            hostCallsBurst: 40,
            eventsPerSec: 10,
            networkRequestsPerSec: 5,
            networkConcurrent: 2,
            deviceSendsPerSec: 3,
            fsOpsPerSec: 7,
            callFuel: 1_000_000,
            callTimeoutMs: 5_000
          }
        }
      })]
    })

    const wrapper = mountPlugins()
    await flushPromises()

    const warning = wrapper.findAllComponents({ name: 'AAlert' }).find(notice => notice.props('message') === messages.plugins.healthLabel('degraded'))
    expect(warning?.props('type')).toBe('warning')
    expect(warning?.props('description')).toBeUndefined()
    // ★ The quota's own numbers, not a second opinion: the rung says how the plugin is doing, these say what it may do.
    // A report with no quota shows no Limits row at all, which the fixture above would catch if the row always rendered.
    expect(wrapper.text()).toContain(messages.plugins.limits)
    expect(wrapper.text()).toContain('64.0 MiB')
    expect(wrapper.text()).toContain('20 calls/s')
    expect(wrapper.text()).toContain('5 s call timeout')
    // ★ And the rest behind the disclosure: the four-number row answers "can it work"; these answer "what exactly did
    // the host decide". The content is in the DOM while the details are closed, which is what makes this readable.
    expect(wrapper.text()).toContain(messages.plugins.allLimits)
    expect(wrapper.text()).toContain(messages.plugins.limitStorage)
    expect(wrapper.text()).toContain('128.0 MiB')
    expect(wrapper.text()).toContain(messages.plugins.limitFuel)
    expect(wrapper.text()).toContain('1000000')
    wrapper.unmount()
  })

  it('says why the host did not start a plugin, instead of only calling it unavailable', async () => {
    // ★ Safe mode: every card is `unavailable` because the host chose not to start any plugin after a crash loop, and a
    // state word alone sends the user looking for a broken artifact. The sentence is the host's own — the page must not
    // invent one — so the assertion is on the exact text it was given.
    const why = 'it crashed 3 times at src/plugin.rs:42; starting without plugins so this can be fixed'
    mocks.call.mockResolvedValue({
      ok: true,
      value: [card({ state: 'unavailable', stateReason: why })]
    })

    const wrapper = mountPlugins()
    await flushPromises()

    expect(wrapper.text()).toContain(why)
    wrapper.unmount()
  })

  it('offers a retry for a faulted plugin and asks the kernel to load it again', async () => {
    // ★ The row is enabled and trusted; what failed was starting it, so turning the switch off and on was the only way
    // to ask again. The button is the difference between "try now" and changing a flag the user did not mean to change.
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_reload')
        return { ok: true, value: card({ state: 'active', enabled: true }) }
      return { ok: true, value: [card({ state: 'faulted', enabled: true, stateReason: 'it failed to start' })] }
    })

    const wrapper = mountPlugins()
    await flushPromises()

    const retry = wrapper.findAll('button').find(candidate => candidate.text() === messages.plugins.retry)
    expect(retry, 'a faulted plugin has no retry').toBeDefined()
    await retry?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('plugins_reload', { pluginId: '@acme/hello' })
    wrapper.unmount()
  })

  it('asks about the data in a dialog before removing a plugin', async () => {
    // ★ The destructive half of removal: the command always accepted `removeData`, and the page sent `false` without ever
    // asking. The dialog renders in place (`get-container=false`), so this test can tick the box that decides it — the
    // argument the action layer sends is covered in `pages/plugins/actions.test.ts`, both ways.
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_uninstall') {
        return {
          ok: true,
          value: { bytesFreed: 1536, dataKept: false, leftovers: ['/tmp/synra/plugins/@acme/hello'] }
        }
      }
      return { ok: true, value: [card()] }
    })
    const wrapper = mountPlugins()
    await flushPromises()

    // The card's button, not the dialog's own footer button: the dialog renders (closed) before the list, so a search by
    // text alone would click the control it is supposed to be opening.
    const remove = wrapper
      .findAll('button')
      .filter(candidate => candidate.classes().includes('ant-btn-dangerous'))
      .at(-1)
    expect(remove, 'the remove button is missing').toBeDefined()
    await remove?.trigger('click')
    await flushPromises()

    const modal = wrapper
      .findAllComponents({ name: 'AModal' })
      .find(candidate => candidate.props('title') === messages.plugins.removeTitle)
    expect(modal, 'the removal dialog is missing').toBeDefined()
    expect(modal?.props('open'), 'the dialog did not open').toBe(true)
    expect(wrapper.text()).toContain(messages.plugins.removeTitle)
    expect(wrapper.text()).toContain(messages.plugins.removeDeleteData)
    // ★ The directory the checkbox would delete: the user is agreeing to delete a path, not an abstraction.
    expect(wrapper.text()).toContain(messages.plugins.removeDataPath(card().dataPath))
    expect(mocks.call).not.toHaveBeenCalledWith('plugins_uninstall', expect.anything())

    const data = wrapper
      .findAllComponents({ name: 'ACheckbox' })
      .find(box => box.text().includes(messages.plugins.removeDeleteData))
    expect(data, 'the delete-data checkbox is missing').toBeDefined()
    await data?.vm.$emit('update:checked', true)

    await modal?.vm.$emit('ok')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('plugins_uninstall', {
      pluginId: '@acme/hello',
      removeData: true
    })
    expect(wrapper.text()).toContain(messages.plugins.removedDeleted('Hello', '1.5 KiB'))
    // ★ And what survived: the row is gone, so the note is the only place a locked directory is reported.
    expect(wrapper.text()).toContain(messages.plugins.leftBehind(1))
    wrapper.unmount()
  })

  it('shows what the plugin asks for, not only what it was given', async () => {
    // The consent checkboxes are built from `requested`; a page that rendered `granted` would show an empty prompt for a
    // plugin nobody had decided about. `granted` is deliberately empty here, so the assertion can only pass through
    // `requested`.
    mocks.call.mockResolvedValue({ ok: true, value: [card({ granted: [], denied: [] })] })

    const wrapper = mountPlugins()
    await flushPromises()

    expect(wrapper.text()).toContain('log:*')
    wrapper.unmount()
  })

  it('refreshes clean permission drafts from store events and preserves unsaved edits', async () => {
    const initial = card({ state: 'active', granted: ['log:*'], requested: ['log:*', 'net:fetch:example.com'] })
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list')
        return { ok: true, value: [initial] }
      if (command === 'plugins_publishers')
        return { ok: true, value: [] }
      if (command === 'plugins_consent_respond')
        return { ok: true, value: { ...initial, granted: ['log:*', 'net:fetch:example.com'] } }
      throw new Error(`unexpected command ${command}`)
    })
    const wrapper = mountPlugins()
    try {
      await flushPromises()
      const save = () => wrapper.findAll('button').find(button => button.text() === messages.plugins.consentAction)
      expect(save()).toBeUndefined()
      const store = usePluginsStore()
      store.upsert({ ...initial, granted: ['net:fetch:example.com'] })
      await flushPromises()
      const choices = wrapper.findComponent({ name: 'ACheckboxGroup' })
      expect(choices.props('value')).toEqual(['net:fetch:example.com'])
      await choices.vm.$emit('update:value', ['log:*', 'net:fetch:example.com'])
      await flushPromises()
      store.upsert({ ...initial, granted: ['net:fetch:example.com'], stateReason: 'Unrelated runtime update' })
      await flushPromises()
      expect(choices.props('value')).toEqual(['log:*', 'net:fetch:example.com'])
      await save()!.trigger('click')
      await flushPromises()
      expect(mocks.call).toHaveBeenCalledWith('plugins_consent_respond', { pluginId: initial.pluginId, granted: ['log:*', 'net:fetch:example.com'] })
      expect(save()).toBeUndefined()
      expect(wrapper.text()).not.toContain(messages.plugins.reviewPermissions)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('shows the refusal when the list cannot be read', async () => {
    mocks.call.mockResolvedValue({ ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } })

    const wrapper = mountPlugins()
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

    const wrapper = mountPlugins()
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

    const wrapper = mountPlugins()
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
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list')
        return { ok: true, value: [card()] }
      if (command === 'plugins_sync_plan')
        return { ok: true, value: plan }
      if (command === 'plugins_sync_run')
        return { ok: true, value: { updated: 1, unchanged: 0 } }
      return { ok: true, value: [] }
    })

    const wrapper = mountPlugins()
    await flushPromises()

    const button = wrapper.findAll('button').find(candidate => candidate.text() === messages.plugins.syncAction)
    expect(button, 'the card drew no sync button').toBeDefined()
    await button?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('plugins_sync_plan', { pluginId: '@acme/hello' })
    expect(mocks.call).toHaveBeenCalledWith('plugins_sync_run', { plan })
    expect(mocks.call.mock.calls.findIndex(([command]) => command === 'plugins_sync_plan'))
      .toBeLessThan(mocks.call.mock.calls.findIndex(([command]) => command === 'plugins_sync_run'))
    expect(wrapper.text()).toContain(messages.plugins.syncDone(1, 0, 0))
    wrapper.unmount()
  })

  it('says there is nothing to send when every device already has it', async () => {
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list')
        return { ok: true, value: [card()] }
      if (command === 'plugins_sync_plan') {
        return {
          ok: true,
          value: {
            pluginId: '@acme/hello',
            targets: [{ deviceId: '11111111-1111-4111-8111-111111111111', displayName: 'Laptop', needsArtifact: false, online: true }]
          }
        }
      }
      return { ok: true, value: [] }
    })

    const wrapper = mountPlugins()
    await flushPromises()

    await wrapper.findAll('button').find(candidate => candidate.text() === messages.plugins.syncAction)?.trigger('click')
    await flushPromises()

    expect(mocks.call).not.toHaveBeenCalledWith('plugins_sync_run', expect.anything())
    expect(wrapper.text()).toContain(messages.plugins.syncNothing)
    wrapper.unmount()
  })

  it('binds publisher decisions to the displayed fingerprint and shows a stale key refusal', async () => {
    const fingerprint = 'AB12 CD34 EF56 7890'
    mocks.call.mockImplementation(async (command: string, args?: { trust?: string }) => {
      if (command === 'plugins_list')
        return { ok: true, value: [] }
      if (command === 'plugins_publishers') {
        return {
          ok: true,
          value: [{ publisherId: '@acme', fingerprint, trust: 'seen', firstSeenMs: 1, decidedMs: null }]
        }
      }
      if (command === 'plugins_publisher_decide') {
        return args?.trust === 'trusted'
          ? { ok: true, value: null }
          : { ok: false, error: { code: 'invalid_argument', message: 'error.invalid_argument', retryable: false } }
      }
      throw new Error(`unexpected command ${command}`)
    })
    const wrapper = mountPlugins()
    await flushPromises()
    expect(wrapper.text()).toContain(fingerprint)
    await wrapper.findAll('button').find(button => button.text() === messages.plugins.publishers.trust)?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('plugins_publisher_decide', { publisherId: '@acme', fingerprint, trust: 'trusted' })
    await wrapper.findAll('button').find(button => button.text() === messages.plugins.publishers.revoke)?.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('plugins_publisher_decide', { publisherId: '@acme', fingerprint, trust: 'revoked' })
    expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('invalid_argument')
    wrapper.unmount()
  })

  it.each(['input', 'source', 'close', 'unmount'])('discards a pending registry preview after %s changes', async (change) => {
    let resolve!: (value: unknown) => void
    const pending = new Promise<unknown>((done) => {
      resolve = done
    })
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_preview')
        return pending
      if (command === 'plugins_list' || command === 'plugins_publishers')
        return { ok: true, value: [] }
      throw new Error(`unexpected command ${command}`)
    })
    const wrapper = mountPlugins({ attachTo: document.body })
    let unmounted = false
    try {
      await flushPromises()
      await wrapper.get('[data-testid="install-toggle"]').trigger('click')
      const mode = wrapper.findComponent({ name: 'ASegmented' })
      await mode.vm.$emit('update:value', 'registry')
      await wrapper.get('#plugin-install input[aria-label]').setValue('@acme/first')
      await wrapper.get('[data-testid="install-submit"]').trigger('click')
      await flushPromises()
      await wrapper.get('#plugin-install input[aria-label]').trigger('keydown', { key: 'Enter', keyCode: 13 })
      expect(mocks.call.mock.calls.filter(([command]) => command === 'plugins_preview')).toHaveLength(1)
      if (change === 'input') {
        await wrapper.get('#plugin-install input[aria-label]').setValue('@acme/second')
      }
      else if (change === 'source') {
        await mode.vm.$emit('update:value', 'path')
      }
      else if (change === 'close') {
        await wrapper.get('[data-testid="install-toggle"]').trigger('click')
      }
      else {
        wrapper.unmount()
        unmounted = true
      }
      resolve({ ok: true, value: { pluginId: '@acme/first', title: 'Old preview', version: '1.0.0', publisher: 'acme', capabilities: [], headless: true, hasUi: false } })
      await flushPromises()
      expect(document.body.textContent).not.toContain('Old preview')
      if (!unmounted) {
        const modal = wrapper.findAllComponents({ name: 'AModal' }).find(modal => modal.props('title') === messages.plugins.preview.title)
        expect(modal?.props('open')).toBe(false)
      }
      expect(mocks.call.mock.calls.filter(([command]) => command === 'plugins_install')).toHaveLength(0)
    }
    finally {
      if (!unmounted)
        wrapper.unmount()
    }
  })

  it('keeps the publisher prompt after a refused decision and allows retry', async () => {
    const fingerprint = 'AB12CD34'
    let trusted = false
    let refuse = true
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list')
        return { ok: true, value: [card()] }
      if (command === 'plugins_publishers')
        return { ok: true, value: [{ publisherId: '@acme', fingerprint, trust: trusted ? 'trusted' : 'seen', firstSeenMs: 0 }] }
      if (command === 'plugins_install')
        return { ok: true, value: { pluginId: '@acme/hello', version: '1.0.0', enabled: false, publisherId: '@acme', fingerprint } }
      if (command === 'plugins_publisher_decide') {
        if (refuse)
          return { ok: false, error: { code: 'unavailable', message: 'error.unavailable', retryable: true } }
        trusted = true
        return { ok: true, value: null }
      }
      throw new Error(`unexpected command ${command}`)
    })
    const wrapper = mountPlugins()
    try {
      await flushPromises()
      await wrapper.get('input[aria-label]').setValue('C:/signed-plugin')
      await wrapper.get('[data-testid="install-submit"]').trigger('click')
      await flushPromises()
      const trust = () => wrapper.findAll('button').find(button => button.text() === messages.plugins.publisherPrompt.trust)!
      await trust().trigger('click')
      await flushPromises()
      expect(wrapper.text()).toContain(messages.plugins.publisherPrompt.title)
      expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('unavailable')
      refuse = false
      await trust().trigger('click')
      await flushPromises()
      expect(wrapper.text()).not.toContain(messages.plugins.publisherPrompt.title)
      expect(mocks.call.mock.calls.filter(([command]) => command === 'plugins_publisher_decide')).toHaveLength(2)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('does not submit registry search while an IME composition is being confirmed', async () => {
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list' || command === 'plugins_publishers' || command === 'plugins_catalog_search')
        return { ok: true, value: [] }
      throw new Error(`unexpected command ${command}`)
    })
    const wrapper = mountPlugins()
    try {
      await flushPromises()
      const input = wrapper.get(`input[placeholder="${messages.plugins.catalog.placeholder}"]`)
      await input.setValue('插件')
      await input.trigger('keydown', { key: 'Enter', keyCode: 13, isComposing: true })
      await input.trigger('keydown', { key: 'Enter', keyCode: 229 })
      await flushPromises()
      expect(mocks.call.mock.calls.filter(([command]) => command === 'plugins_catalog_search')).toHaveLength(0)
      await input.trigger('keydown', { key: 'Enter', keyCode: 13 })
      await flushPromises()
      expect(mocks.call).toHaveBeenCalledWith('plugins_catalog_search', { query: { query: '插件', offset: 0, sort: 'relevance' } })
    }
    finally {
      wrapper.unmount()
    }
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

    const wrapper = mountPlugins()
    await flushPromises()

    const mode = wrapper.findComponent({ name: 'ASegmented' })
    await mode.vm.$emit('update:value', 'registry')
    await flushPromises()
    await wrapper.find('input[placeholder="Plugin id, e.g. @acme/hello"]').setValue('@acme/hello')
    const install = wrapper.get('[data-testid="install-submit"]')
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

    const wrapper = mountPlugins()
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

  it('shows the update the registry publishes when asked', async () => {
    // ★ The check is a user action, not a mount-time fan-out: one request per plugin on every page open would make the
    // list depend on the registry being reachable.
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list')
        return { ok: true, value: [card()] }
      if (command === 'plugins_publishers')
        return { ok: true, value: [] }
      if (command === 'plugins_check_update') {
        return { ok: true, value: { version: '2.0.0', needsNewConsent: true, addedCapabilities: ['device:send:*'] } }
      }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })

    const wrapper = mountPlugins()
    await flushPromises()

    const check = wrapper.findAll('button').find(button => button.text().includes('Check for updates'))
    expect(check).toBeDefined()
    await check?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('plugins_check_update', { pluginId: '@acme/hello' })
    expect(wrapper.text()).toContain('Version 2.0.0 is published')
    expect(wrapper.text()).toContain('device:send:*')
    wrapper.unmount()
  })

  it('keeps the update report on the page after the toast is gone', async () => {
    // ★ The command has returned the report since §8.221; the page threw it away. A toast says the same thing for a few
    // seconds, and "the package is not signed" is exactly the sentence a person wants to re-read.
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list')
        return { ok: true, value: [card()] }
      if (command === 'plugins_publishers')
        return { ok: true, value: [] }
      if (command === 'plugins_check_update')
        return { ok: true, value: { version: '2.0.0', needsNewConsent: false, addedCapabilities: [] } }
      if (command === 'plugins_update') {
        return {
          ok: true,
          value: {
            pluginId: '@acme/hello',
            version: '2.0.0',
            warnings: ['the package is not signed'],
            enabled: false
          }
        }
      }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })

    const wrapper = mountPlugins()
    await flushPromises()

    await wrapper.findAll('button').find(button => button.text().includes('Check for updates'))?.trigger('click')
    await flushPromises()
    await wrapper.findAll('button').find(button => button.text() === 'Update')?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('plugins_update', { pluginId: '@acme/hello' })
    expect(wrapper.text()).toContain('Updated @acme/hello to 2.0.0')
    expect(wrapper.text()).toContain('the package is not signed')
    wrapper.unmount()
  })

  it('shows the updates the background pass found without checking again', async () => {
    // ★ The daily pass runs whether or not this page is open. Reading its cached answer is what turns "2 updates are
    // available" into buttons on the right cards; asking the registry again on mount would make opening the page a
    // network operation and the answer could disagree with the notice.
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list')
        return { ok: true, value: [card({ update: { pluginId: '@acme/hello', version: '2.0.0', needsNewConsent: false, addedCapabilities: [] } })] }
      if (command === 'plugins_publishers')
        return { ok: true, value: [] }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })

    const wrapper = mountPlugins()
    await flushPromises()

    expect(mocks.call).not.toHaveBeenCalledWith('plugins_check_update', expect.anything())
    expect(wrapper.text()).toContain('Version 2.0.0 is published')
    wrapper.unmount()
  })

  it('shows a background answer while open and clears an update when the next answer is negative', async () => {
    mocks.call.mockImplementation(async (command: string) => ({
      ok: true,
      value: command === 'plugins_list' ? [card()] : []
    }))
    const wrapper = mountPlugins()
    await flushPromises()
    const store = usePluginsStore()
    const checkedAt = 1_700_000_000_000
    store.apply({
      type: 'pluginUpdated',
      payload: card({
        update: { pluginId: '@acme/hello', version: '2.0.0', needsNewConsent: true, addedCapabilities: ['device:send:*'] },
        updateCheckedAtMs: checkedAt
      })
    })
    await flushPromises()
    expect(wrapper.text()).toContain('Version 2.0.0 is published')
    expect(wrapper.text()).toContain('device:send:*')
    expect(wrapper.text()).toContain(messages.plugins.lastChecked.replace('{time}', formatDateTime(checkedAt)))
    expect(wrapper.findAll('button').some(button => button.text() === messages.plugins.updateAction)).toBe(true)

    store.apply({ type: 'pluginUpdated', payload: card({ updateCheckedAtMs: checkedAt + 1 }) })
    await flushPromises()
    expect(wrapper.text()).not.toContain('Version 2.0.0 is published')
    expect(wrapper.findAll('button').some(button => button.text() === messages.plugins.updateAction)).toBe(false)
    expect(mocks.call).not.toHaveBeenCalledWith('plugins_check_update', expect.anything())
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

    const wrapper = mountPlugins()
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
    const wrapper = mountPlugins()
    await flushPromises()

    usePluginsStore().upsert(card({ title: 'Arrived by event' }))
    await flushPromises()

    expect(wrapper.text()).toContain('Arrived by event')
    wrapper.unmount()
  })

  it('reads a plugins recent calls when the trail is opened', async () => {
    // ★ The trail is pulled, not read on mount: one ring per plugin on every page open is work nobody asked for. What
    // this asserts is the pull — the closed section asks nothing, and opening it reaches the command and draws the rows.
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list')
        return { ok: true, value: [card()] }
      if (command === 'plugins_publishers')
        return { ok: true, value: [] }
      if (command === 'plugins_audit') {
        // Newest first, as the table's `ORDER BY at_ms DESC, id DESC` answers: the page draws the list as it arrives.
        return {
          ok: true,
          value: [
            { atMs: 1_700_000_001_000, capability: 'network:fetch', allowed: false, reason: 'denied' },
            {
              atMs: 1_700_000_000_000,
              capability: 'log:write',
              allowed: true,
              traceId: '01a150c8-c332-768d-9dd1-981502aa1854',
              elapsedMs: 1.25
            }
          ]
        }
      }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })

    const wrapper = mountPlugins()
    await flushPromises()

    expect(mocks.call).not.toHaveBeenCalledWith('plugins_audit', expect.anything())
    // Nothing to clear yet, so no control for it: the button's absence is part of the same "not asked" state.
    expect(wrapper.findAll('button').some(candidate => candidate.text() === messages.plugins.audit.clear)).toBe(false)
    await wrapper.get('[data-plugin-management] .ant-collapse-header').trigger('click')
    await flushPromises()
    const trail = wrapper.find('[data-plugin-audit]')
    expect(trail.exists()).toBe(true)
    ;(trail.element as HTMLDetailsElement).open = true
    // ★ Setting `open` queues the browser's own `toggle` event; dispatching a second one by hand would read the trail
    // twice and make "the page read it once" untestable. The wait is for that queued task.
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('plugins_audit', { pluginId: '@acme/hello', limit: 20, offset: 0 })
    expect(wrapper.text()).toContain('log:write')
    // ★ The refusal is named, not just coloured: `denied` is the gate's own outcome name, and a row without it tells the
    // user a call failed without telling them why.
    expect(wrapper.text()).toContain('network:fetch')
    expect(wrapper.text()).toContain(messages.plugins.audit.refused)
    expect(wrapper.text()).toContain('denied')
    const rows = wrapper.findAll('details li')
    expect(rows[0]?.text()).toContain('network:fetch')
    // ★ The action's trace is shown on the row that has one, and absent from the row that does not. It is the id a user
    // pastes into `synra logs --trace`; a row without it means "this call had no action behind it", not a missing lookup.
    expect(rows[1]?.text()).toContain('01a150c8-c332-768d-9dd1-981502aa1854')
    expect(rows[1]?.text()).toContain(messages.plugins.audit.elapsed(1.25))
    expect(rows[1]?.get('time').text()).toBe('2023-11-14T22:13:20.000Z')
    expect(rows[0]?.text()).not.toContain('01a150c8-c332-768d-9dd1-981502aa1854')
    // A two-row page cannot have another one, so the control for it is absent.
    expect(wrapper.findAll('button').some(candidate => candidate.text() === messages.plugins.audit.more)).toBe(false)
    wrapper.unmount()
  })

  it('appends the next page of calls without repeating the first', async () => {
    // ★ "Show more" is an offset into the same newest-first order, not a second query. The page appends; the button goes
    // away when a page comes back short, which is the only signal a limit-only read gives that there is no more.
    const page = (start: number, count: number) =>
      Array.from({ length: count }, (_, index) => ({
        atMs: start + index,
        capability: 'log:write',
        allowed: true
      }))
    mocks.call.mockImplementation(async (command: string, args?: { offset?: number }) => {
      if (command === 'plugins_list')
        return { ok: true, value: [card()] }
      if (command === 'plugins_publishers')
        return { ok: true, value: [] }
      if (command === 'plugins_audit')
        return { ok: true, value: args?.offset === 20 ? page(101, 4) : page(1, 20) }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })

    const wrapper = mountPlugins()
    await flushPromises()

    await wrapper.get('[data-plugin-management] .ant-collapse-header').trigger('click')
    await flushPromises()
    const trail = wrapper.find('[data-plugin-audit]')
    ;(trail.element as HTMLDetailsElement).open = true
    await flushPromises()

    expect(wrapper.findAll('details li')).toHaveLength(20)
    const more = wrapper.findAll('button').find(candidate => candidate.text() === messages.plugins.audit.more)
    expect(more, 'a full page drew no way to read the rest').toBeDefined()
    await more?.trigger('click')
    await flushPromises()

    expect(mocks.call).toHaveBeenCalledWith('plugins_audit', { pluginId: '@acme/hello', limit: 20, offset: 20 })
    expect(wrapper.findAll('details li')).toHaveLength(24)
    expect(wrapper.findAll('button').some(candidate => candidate.text() === messages.plugins.audit.more)).toBe(false)
    wrapper.unmount()
  })

  it('shows a refusal to read the trail rather than an empty one', async () => {
    // ★ "Nothing recorded" and "could not be read" are different answers: an empty section on a refusal would tell the
    // user a plugin has been quiet when the shell simply could not ask.
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list')
        return { ok: true, value: [card()] }
      if (command === 'plugins_publishers')
        return { ok: true, value: [] }
      if (command === 'plugins_audit')
        return { ok: false, error: { code: 'unsupported', message: 'error.unsupported', retryable: false } }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })

    const wrapper = mountPlugins()
    await flushPromises()

    await wrapper.get('[data-plugin-management] .ant-collapse-header').trigger('click')
    await flushPromises()
    const trail = wrapper.find('[data-plugin-audit]')
    ;(trail.element as HTMLDetailsElement).open = true
    // Setting `open` queues the browser's own `toggle` event; a second, hand-dispatched one would read the trail twice.
    await flushPromises()

    expect(wrapper.findComponent(ErrorNotice).props('code')).toBe('unsupported')
    expect(wrapper.text()).not.toContain(messages.plugins.audit.empty)
    wrapper.unmount()
  })

  it('keeps failed clearing local to its plugin and retries without another read', async () => {
    let refuse = true
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list')
        return { ok: true, value: [card()] }
      if (command === 'plugins_publishers')
        return { ok: true, value: [] }
      if (command === 'plugins_audit')
        return { ok: true, value: [{ atMs: 0, capability: 'saved-record', allowed: true }] }
      if (command === 'plugins_audit_clear')
        return refuse ? { ok: false, error: { code: 'unavailable', message: 'error.unavailable', retryable: true, actions: [{ kind: 'retry' }] } } : { ok: true, value: null }
      throw new Error(`unexpected command ${command}`)
    })
    const wrapper = mountPlugins({ attachTo: document.body })
    try {
      await flushPromises()
      await wrapper.get('[data-plugin-management] .ant-collapse-header').trigger('click')
      await flushPromises()
      const trail = wrapper.get('[data-plugin-audit]')
      ;(trail.element as HTMLDetailsElement).open = true
      await flushPromises()
      await trail.findAll('button').find(button => button.text() === messages.plugins.audit.clear)!.trigger('click')
      await flushPromises()
      await trail.findAll('button').find(button => button.text() === messages.plugins.audit.confirmClear)!.trigger('click')
      await flushPromises()
      expect(trail.findComponent(ErrorNotice).props('code')).toBe('unavailable')
      expect(trail.text()).toContain('saved-record')
      expect(trail.text()).toContain(messages.plugins.audit.clearDetail)
      await trail.findAll('button').find(button => button.text() === messages.plugins.audit.cancel)!.trigger('click')
      await flushPromises()
      await trail.findAll('button').find(button => button.text() === messages.actions.retry)!.trigger('click')
      await flushPromises()
      expect(trail.text()).toContain(messages.plugins.audit.clearDetail)
      expect(mocks.call.mock.calls.filter(([command]) => command === 'plugins_audit_clear')).toHaveLength(1)
      refuse = false
      await trail.findAll('button').find(button => button.text() === messages.actions.retry)!.trigger('click')
      await flushPromises()
      expect(trail.findComponent(ErrorNotice).exists()).toBe(false)
      expect(trail.text()).not.toContain('saved-record')
      expect(trail.text()).not.toContain(messages.plugins.audit.clearDetail)
      expect(document.activeElement).toBe(trail.get('summary').element)
      expect(mocks.call.mock.calls.filter(([command]) => command === 'plugins_audit')).toHaveLength(1)
      expect(mocks.call.mock.calls.filter(([command]) => command === 'plugins_audit_clear')).toHaveLength(2)
    }
    finally {
      wrapper.unmount()
    }
  })

  it('clears the history from the card that drew it', async () => {
    // ★ The clear is the other half of the section, and what it must not do is re-read: the command removed every record,
    // so the page already knows the answer. Clearing the local list is also what makes the empty sentence appear without a
    // second round trip that could disagree with the command that just succeeded.
    let cleared = false
    mocks.call.mockImplementation(async (command: string) => {
      if (command === 'plugins_list')
        return { ok: true, value: [card()] }
      if (command === 'plugins_publishers')
        return { ok: true, value: [] }
      if (command === 'plugins_audit') {
        return {
          ok: true,
          value: cleared ? [] : [{ atMs: 1_700_000_000_000, capability: 'log:write', allowed: true }]
        }
      }
      if (command === 'plugins_audit_clear') {
        cleared = true
        return { ok: true, value: null }
      }
      return { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
    })

    const wrapper = mountPlugins({ attachTo: document.body })
    await flushPromises()

    await wrapper.get('[data-plugin-management] .ant-collapse-header').trigger('click')
    await flushPromises()
    const trail = wrapper.find('[data-plugin-audit]')
    ;(trail.element as HTMLDetailsElement).open = true
    // Setting `open` queues the browser's own `toggle` event; a second, hand-dispatched one would read the trail twice.
    await flushPromises()

    const clear = wrapper.findAll('button').find(candidate => candidate.text() === messages.plugins.audit.clear)
    expect(clear, 'the section with rows drew no way to clear them').toBeDefined()
    await clear?.trigger('click')
    await flushPromises()

    expect(mocks.call).not.toHaveBeenCalledWith('plugins_audit_clear', expect.anything())
    const cancel = wrapper.findAll('button').find(button => button.text() === messages.plugins.audit.cancel)!
    expect(document.activeElement).toBe(cancel.element)
    await cancel.trigger('click')
    await flushPromises()
    expect(document.activeElement).toBe(clear?.element)
    expect(mocks.call).not.toHaveBeenCalledWith('plugins_audit_clear', expect.anything())
    await clear?.trigger('click')
    await flushPromises()
    await wrapper.findAll('button').find(button => button.text() === messages.plugins.audit.confirmClear)!.trigger('click')
    await flushPromises()
    expect(mocks.call).toHaveBeenCalledWith('plugins_audit_clear', { pluginId: '@acme/hello' })
    expect(document.activeElement).toBe(trail.get('summary').element)
    expect(wrapper.text()).not.toContain('log:write')
    expect(wrapper.text()).toContain(messages.plugins.audit.empty)
    // The read happened once, before the clear; a second one would be a round trip for a list that is now empty.
    expect(mocks.call.mock.calls.filter(([command]) => command === 'plugins_audit')).toHaveLength(1)
    wrapper.unmount()
  })
})
