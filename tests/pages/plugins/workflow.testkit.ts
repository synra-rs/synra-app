import type { PluginSnapshot } from '@synra/protocol'
import type { VueWrapper } from '@vue/test-utils'
import { flushPromises, mount } from '@vue/test-utils'
import { Button } from 'ant-design-vue'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, vi } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setLocalePreference } from '@/app/locale'
import { installBrowserApis } from '@/testing/browser'

const mocks = vi.hoisted(() => ({ call: vi.fn(), open: vi.fn() }))
export { mocks }
vi.mock('@/app/kernel', () => ({ kernel: { call: mocks.call } }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: mocks.open }))
export const PluginsPage = (await import('@/pages/plugins.vue')).default
export const PluginRow = (await import('@/components/plugins/plugin-row.vue')).default
export const PluginDetail = (await import('@/components/plugins/plugin-detail.vue')).default
export const PluginPermissions = (await import('@/components/plugins/plugin-permissions.vue')).default
export const LocalInstallDialog = (await import('@/components/plugins/local-install-dialog.vue')).default
export const RegistryDiscover = (await import('@/components/plugins/registry-discover.vue')).default
export const RegistryInstallDialog = (await import('@/components/plugins/registry-install-dialog.vue')).default
export const PluginRemoval = (await import('@/components/plugins/plugin-removal.vue')).default
export const PairingPrompt = (await import('@/components/PairingPrompt.vue')).default
export const { usePairingStore } = await import('@/stores/pairing')
export const ErrorNotice = (await import('@/components/ErrorNotice.vue')).default
export const { usePluginsStore } = await import('@/stores/plugins')
export const { messages } = await import('@/messages')

export function card(overrides: Partial<PluginSnapshot> = {}): PluginSnapshot {
  return { pluginId: '@acme/reader', dataPath: 'data/reader', title: 'Reader', description: 'A plugin with a page', version: '1.0.0', publisher: '@acme', enabled: true, state: 'active', hasUi: true, hybrid: false, requested: ['log:*', 'net:fetch:example.com'], granted: ['log:*'], denied: ['net:fetch:example.com'], ...overrides }
}
export function answer(byCommand: Record<string, unknown> = {}): void {
  mocks.call.mockImplementation(async (command: string, args: unknown) => {
    const defaults: Record<string, unknown> = { plugins_list: [card()], plugins_publishers: [], plugins_catalog_search: [], plugins_audit: [], plugins_check_update: null }
    const value = command in byCommand ? byCommand[command] : defaults[command]
    if (!(command in byCommand) && !(command in defaults))
      throw new Error(`Unexpected command: ${command}`)
    return typeof value === 'function' ? value(args) : { ok: true, value }
  })
}
const mounted: VueWrapper[] = []
beforeEach(() => {
  installBrowserApis()
  setActivePinia(createPinia())
  setLocalePreference('en')
  mocks.call.mockReset()
  mocks.open.mockReset()
  document.body.innerHTML = ''
  answer()
})
afterEach(() => {
  for (const wrapper of mounted.splice(0))
    wrapper.unmount()
  setLocalePreference('system')
})
export async function mountPage(path = '/plugins') {
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/plugins', component: PluginsPage }, { path: '/settings', component: { template: '<div />' } }] })
  await router.push(path)
  const wrapper = mount(PluginsPage, { attachTo: document.body, global: { plugins: [router] } })
  mounted.push(wrapper)
  await flushPromises()
  return { wrapper, router }
}
export async function click(wrapper: Pick<VueWrapper, 'findAllComponents'>, text: string): Promise<void> {
  const button = await vi.waitFor(() => {
    const found = wrapper.findAllComponents(Button).find(button => button.text().replace(/\s/g, '') === text.replace(/\s/g, ''))
    if (!found)
      throw new Error(`Button not found: ${text}`)
    return found
  })
  await button.trigger('click')
  await flushPromises()
}
export async function clickBody(text: string, role = 'button'): Promise<void> {
  const node = [...document.querySelectorAll<HTMLElement>(role === 'button' ? 'button' : `[role="${role}"]`)].find(node => node.textContent?.replace(/\s/g, '') === text.replace(/\s/g, ''))
  if (!node)
    throw new Error(`Visible control not found: ${text}`)
  node.click()
  await flushPromises()
}
