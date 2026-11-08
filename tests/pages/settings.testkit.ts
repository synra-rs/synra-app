import type { VueWrapper } from '@vue/test-utils'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMemoryHistory, createRouter, RouterView } from 'vue-router'
import { localePreference, setLocalePreference } from '@/app/locale'
import { installBrowserApis } from '@/testing/browser'

const mocks = vi.hoisted(() => ({
  call: vi.fn(),
  subscribe: vi.fn()
}))
const autostart = vi.hoisted(() => ({
  isEnabled: vi.fn(),
  enable: vi.fn(),
  disable: vi.fn()
}))
vi.mock('@/app/kernel', () => ({ kernel: { call: mocks.call, subscribe: mocks.subscribe } }))
vi.mock('@tauri-apps/plugin-autostart', () => ({
  isEnabled: autostart.isEnabled,
  enable: autostart.enable,
  disable: autostart.disable
}))
const SettingsPage = (await import('@/pages/settings.vue')).default
const { IDENTITY } = await import('@/ipc/fake-kernel-fixtures')
const ErrorNotice = (await import('@/components/ErrorNotice.vue')).default
const { messages } = await import('@/messages')

const NO_METRICS = { values: [], overflowedTotal: 0, maxSeriesPerMetric: 1000 }
function answer(byCommand: Record<string, unknown>) {
  const answers: Record<string, unknown> = { diag_metrics: NO_METRICS, ...byCommand }
  mocks.call.mockImplementation(async (command: string, args?: {
    patch?: {
      set?: Record<string, unknown>
      unset?: string[]
    }
  }) => {
    if (command === 'config_set' && command in answers) {
      const snapshot = answers.config_get as {
        values: Record<string, unknown>
        overridden: string[]
      } | undefined
      if (snapshot) {
        Object.assign(snapshot.values, args?.patch?.set)
        for (const key of args?.patch?.unset ?? [])
          delete snapshot.values[key]
        snapshot.overridden = Object.keys(snapshot.values)
      }
    }
    return command in answers
      ? { ok: true, value: answers[command] }
      : { ok: false, error: { code: 'internal', message: 'error.internal', retryable: false } }
  })
}
let routerInstance: ReturnType<typeof createRouter>
function testRouter() {
  return routerInstance
}
async function mountPage(path = '/settings') {
  routerInstance = createRouter({ history: createMemoryHistory(), routes: [{ path: '/settings', component: SettingsPage }, { path: '/', component: { template: '<p>Home</p>' } }] })
  await routerInstance.push(path)
  await routerInstance.isReady()
  return mount(RouterView, { attachTo: document.body, global: { plugins: [routerInstance] } })
}
async function openCategory(wrapper: VueWrapper, category: string): Promise<void> {
  const select = wrapper.findAllComponents({ name: 'ASelect' }).find(value => value.attributes('data-testid') === 'settings-category')!
  await select.vm.$emit('change', category)
  await flushPromises()
}
it('names every diagnostics panel without the library arrow word', async () => {
  answer({ config_get: { values: {}, overridden: [] }, identity_get: IDENTITY })
  const wrapper = await mountPage('/settings?category=diagnostics')
  try {
    await flushPromises()
    const headers = wrapper.findAll('[data-testid="settings-diagnostics"] .ant-collapse-header')
    expect(headers.length).toBeGreaterThan(0)
    for (const header of headers) {
      for (const image of header.findAll('[role="img"]'))
        expect(image.attributes('aria-label')).toBeUndefined()
    }
  }
  finally {
    wrapper.unmount()
  }
})
async function openDiagnostic(wrapper: VueWrapper, title: string): Promise<void> {
  await openCategory(wrapper, 'diagnostics')
  const header = wrapper.findAll('[data-testid="settings-diagnostics"] .ant-collapse-header').find(value => value.text().includes(title))!
  await header.trigger('click')
  await flushPromises()
}
beforeEach(() => {
  setLocalePreference('system')
  installBrowserApis()
  setActivePinia(createPinia())
  mocks.call.mockReset()
  mocks.subscribe.mockReset()
  mocks.subscribe.mockResolvedValue(() => { })
  autostart.isEnabled.mockReset()
  autostart.isEnabled.mockResolvedValue(false)
  autostart.enable.mockReset()
  autostart.enable.mockResolvedValue(undefined)
  autostart.disable.mockReset()
  autostart.disable.mockResolvedValue(undefined)
})
export { afterEach, answer, autostart, beforeEach, createMemoryHistory, createPinia, createRouter, describe, ErrorNotice, expect, flushPromises, IDENTITY, installBrowserApis, it, localePreference, messages, mocks, mount, mountPage, NO_METRICS, openCategory, openDiagnostic, RouterView, setActivePinia, setLocalePreference, SettingsPage, testRouter, vi }
export type { VueWrapper }
