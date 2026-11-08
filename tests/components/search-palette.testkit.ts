import type { PluginSnapshot } from '@synra/protocol'
import type { VueWrapper } from '@vue/test-utils'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'

const SearchPalette = (await import('@/components/SearchPalette.vue')).default
const { usePluginsStore } = await import('@/stores/plugins')
const { useDevicesStore } = await import('@/stores/devices')
const { messages } = await import('@/messages')

function snapshot(overrides: Partial<PluginSnapshot> = {}): PluginSnapshot {
  return {
    pluginId: '@acme/starter',
    dataPath: 'plugins/acme/starter',
    title: 'Starter',
    description: 'Examples.',
    version: '0.1.0',
    publisher: 'Acme',
    enabled: true,
    state: 'running',
    stateReason: undefined,
    hybrid: false,
    hasUi: true,
    health: 'ok',
    granted: [],
    requested: [],
    denied: [],
    update: undefined,
    updateCheckedAtMs: undefined,
    ...overrides
  } as PluginSnapshot
}

const mounted: VueWrapper[] = []
afterEach(() => {
  for (const wrapper of mounted.splice(0))
    wrapper.unmount()
})

function mountPalette() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', component: { template: '<div />' } },
      { path: '/devices', component: { template: '<div />' } },
      { path: '/plugins', component: { template: '<div />' } },
      { path: '/settings', component: { template: '<div />' } },
      { path: '/:pathMatch(.*)*', component: { template: '<div />' } }
    ]
  })
  const wrapper = mount(SearchPalette, { global: { plugins: [router] }, attachTo: document.body })
  mounted.push(wrapper)
  return { router, wrapper }
}

function field(): HTMLInputElement {
  const found = document.querySelector<HTMLInputElement>('input[role="combobox"]')
  if (!found)
    throw new Error('the search field is not on the page')
  return found
}

function options(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>('[role="option"]'))
}

function selected(): HTMLElement[] {
  return Array.from(document.querySelectorAll<HTMLElement>('[aria-selected="true"]'))
}

function text(): string {
  return document.body.textContent ?? ''
}

async function open(wrapper: ReturnType<typeof mountPalette>['wrapper']): Promise<void> {
  ;(wrapper.vm as unknown as { show: () => void }).show()
  await flushPromises()
}

async function type(text: string): Promise<void> {
  const input = field()
  input.value = text
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await flushPromises()
}

beforeEach(() => {
  setActivePinia(createPinia())
  document.body.innerHTML = ''
})
export { field, messages, mountPalette, open, options, selected, snapshot, text, type, useDevicesStore, usePluginsStore }
