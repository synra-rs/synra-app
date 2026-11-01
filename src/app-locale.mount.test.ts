// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { ConfigProvider } from 'ant-design-vue'
import { createPinia } from 'pinia'
import { afterEach, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import App from './App.vue'
import { IDENTITY } from './ipc/fake-kernel-fixtures'
import { setLocalePreference } from './locale'
import { messages } from './messages'
import { useSettingsStore } from './stores/settings'

const { call } = vi.hoisted(() => ({ call: vi.fn() }))
vi.mock('./kernel', () => ({ kernel: { call } }))

afterEach(() => {
  setLocalePreference('system')
  vi.restoreAllMocks()
})

it('restores language before visiting settings and updates navigation, document and controls together', async () => {
  window.matchMedia = vi.fn().mockReturnValue({ matches: true, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} })
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} }
  call.mockImplementation(async (command: string) => {
    if (command === 'config_get')
      return { ok: true, value: { values: { 'ui.locale': 'zh-CN' }, overridden: ['ui.locale'] } }
    if (command === 'devices_snapshot')
      return { ok: true, value: { identity: IDENTITY, devices: [], scanning: undefined } }
    throw new Error(`Unexpected command: ${command}`)
  })
  const pinia = createPinia()
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/', component: { template: '<div />' } }] })
  await router.push('/')
  const wrapper = mount(App, { global: { plugins: [pinia, router], stubs: { CrashNotice: true, NoticeHost: true, TransferPrompt: true, RouterView: true } } })
  try {
    await flushPromises()
    expect(call).toHaveBeenCalledWith('config_get')
    expect(wrapper.text()).toContain('首页')
    expect(wrapper.text()).toContain('设置')
    expect(document.documentElement.lang).toBe('zh-CN')
    expect(wrapper.findComponent(ConfigProvider).props('locale')?.locale).toBe('zh-cn')
    useSettingsStore(pinia).apply({ type: 'configUpdated', payload: { values: { 'ui.locale': 'en' }, overridden: ['ui.locale'] } })
    await flushPromises()
    expect(wrapper.text()).toContain('Home')
    expect(wrapper.text()).toContain('Settings')
    expect(document.documentElement.lang).toBe('en')
    expect(wrapper.findComponent(ConfigProvider).props('locale')?.locale).toBe('en')
  }
  finally {
    wrapper.unmount()
  }
})

it('gives the search entry a full-size target', async () => {
  // ★ `09 §3` sets 44px as the floor. This button had only `py-1`, so its height came entirely from its contents and
  // an icon size change would have moved the target. Asserted on the class: jsdom has no layout.
  //
  // ★ `md:min-h-8` as well, matching `DeviceRow` and `SettingsRow`: the floor is for a thumb, not a desktop pointer.
  window.matchMedia = vi.fn().mockReturnValue({ matches: true, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} })
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} }
  call.mockImplementation(async (command: string) => {
    if (command === 'config_get')
      return { ok: true, value: { values: {}, overridden: [] } }
    if (command === 'devices_snapshot')
      return { ok: true, value: { identity: IDENTITY, devices: [], scanning: undefined } }
    throw new Error(`Unexpected command: ${command}`)
  })
  const pinia = createPinia()
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/', component: { template: '<div />' } }] })
  await router.push('/')
  const wrapper = mount(App, { global: { plugins: [pinia, router], stubs: { CrashNotice: true, NoticeHost: true, TransferPrompt: true, RouterView: true } } })
  try {
    await flushPromises()
    const entry = wrapper.get('[data-testid="search-entry"]')
    expect(entry.classes()).toContain('min-h-11')
    // The desktop drop, so this entry is not taller than the nav items beside it.
    expect(wrapper.html()).toContain('md:min-h-8')
  }
  finally {
    wrapper.unmount()
  }
})

it('names the page menu, so its four links are not offered from nowhere', async () => {
  // ★ `AMenu` renders `<ul role="menu">` with no accessible name of its own. Without one a screen reader offers
  //   "Home", "Devices", "Plugins", "Settings" as four links with nothing saying they are this window's pages —
  //   `bsk_inspect`'s tree showed a bare `menu` above them. The name is `messages.app.navigation`, the same object the
  //   four labels come from, so a page rename cannot leave the name behind.
  window.matchMedia = vi.fn().mockReturnValue({ matches: true, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} })
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} }
  call.mockImplementation(async (command: string) => {
    if (command === 'config_get')
      return { ok: true, value: { values: {}, overridden: [] } }
    if (command === 'devices_snapshot')
      return { ok: true, value: { identity: IDENTITY, devices: [], scanning: undefined } }
    throw new Error(`Unexpected command: ${command}`)
  })
  const pinia = createPinia()
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/', component: { template: '<div />' } }] })
  await router.push('/')
  const wrapper = mount(App, { global: { plugins: [pinia, router], stubs: { CrashNotice: true, NoticeHost: true, TransferPrompt: true, RouterView: true } } })
  try {
    await flushPromises()
    const menu = wrapper.findAll('[role="menu"]').find(candidate => candidate.find('[role="menuitem"]').exists())
    expect(menu, 'the page menu is missing').toBeDefined()
    expect(menu?.attributes('aria-label')).toBe(messages.app.navigation)
  }
  finally {
    wrapper.unmount()
  }
})
it('still names the device when the sidebar is a rail', async () => {
  // ★ The rail hides `displayName`, `platform` and the version. That leaves an `AAvatar` holding one
  //   `aria-hidden` laptop glyph — an unnamed square, to a screen reader and to the eye. The name moves onto
  //   the avatar instead of disappearing with the text beside it.
  window.matchMedia = vi.fn().mockReturnValue({ matches: false, addEventListener() {}, removeEventListener() {} })
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} }
  call.mockImplementation(async (command: string) => {
    if (command === 'config_get')
      return { ok: true, value: { values: {}, overridden: [] } }
    if (command === 'devices_snapshot')
      return { ok: true, value: { identity: IDENTITY, devices: [], scanning: undefined } }
    throw new Error(`Unexpected command: ${command}`)
  })
  const pinia = createPinia()
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/', component: { template: '<div />' } }] })
  await router.push('/')
  const wrapper = mount(App, { global: { plugins: [pinia, router], stubs: { CrashNotice: true, NoticeHost: true } } })
  try {
    await flushPromises()
    // The rail is the state where the name is gone from the text.
    expect(wrapper.text()).not.toContain(IDENTITY.displayName)
    const avatar = wrapper.find('.ant-avatar')
    expect(avatar.exists(), 'the device avatar is missing').toBe(true)
    expect(avatar.attributes('aria-label'), 'the rail avatar names nothing').toBe(IDENTITY.displayName)
    expect(avatar.attributes('title'), 'no hover name either').toBe(IDENTITY.displayName)
  }
  finally {
    wrapper.unmount()
  }
})
