// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { ConfigProvider } from 'ant-design-vue'
import { createPinia } from 'pinia'
import { afterEach, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import App from './App.vue'
import { IDENTITY } from './ipc/fake-kernel-fixtures'
import { setLocalePreference } from './locale'
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
