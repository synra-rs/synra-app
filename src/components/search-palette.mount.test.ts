// @vitest-environment jsdom
/**
 * The shell's search box.
 *
 * Three things it can get wrong, and each is asserted rather than described: `/` must not steal a slash from a field
 * (the plugin filter is where a person types, and a shortcut that also fires there makes `/` untypable), a query must
 * come back grouped rather than as one undifferentiated list, and choosing a result must go to the object it named —
 * a settings section to that section, not merely to the settings page.
 */

import type { PluginSnapshot } from '@synra/protocol'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'

const SearchPalette = (await import('./SearchPalette.vue')).default
const { usePluginsStore } = await import('../stores/plugins')
const { useDevicesStore } = await import('../stores/devices')
const { messages } = await import('../messages')

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
  return { router, wrapper }
}

/**
 * The box is inside an antd `Modal`, which teleports to `document.body` — so it is read from the document, not from
 * the wrapper. `attached` is checked first because a missing modal and an empty one fail the same way otherwise.
 */
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

/** Opens the box the way the sidebar entry does, so the test does not depend on the shortcut. */
async function open(wrapper: ReturnType<typeof mountPalette>['wrapper']): Promise<void> {
  ;(wrapper.vm as unknown as { show: () => void }).show()
  await flushPromises()
}

/** Sets the query through the input, which is the path a keystroke takes. */
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

describe('the search box', () => {
  it('finds a plugin by title and by id', async () => {
    usePluginsStore().applySnapshot([snapshot()])

    const { wrapper } = mountPalette()
    await open(wrapper)

    await type('start')
    expect(text()).toContain('Starter')

    // The id is what a person copies out of a log, so it has to match too.
    await type('@acme')
    expect(text()).toContain('Starter')
  })

  it('groups what it found instead of returning one flat list', async () => {
    usePluginsStore().applySnapshot([snapshot()])

    const { wrapper } = mountPalette()
    await open(wrapper)

    // "s" is in several settings section labels and in every page word; both headings must appear.
    await type('s')
    expect(text()).toContain(messages.search.groups.settings)
    expect(text()).toContain(messages.search.groups.pages)
  })

  it('says so when nothing matches', async () => {
    const { wrapper } = mountPalette()
    await open(wrapper)

    await type('zzzz-nothing')
    expect(text()).toContain(messages.search.empty)
  })

  it('takes a settings section to that section, not just to the page', async () => {
    const { router, wrapper } = mountPalette()
    await open(wrapper)

    await type(messages.settings.categories.diagnostics)
    const found = options()
    expect(found.length).toBeGreaterThan(0)
    found[0]!.click()
    await flushPromises()

    expect(router.currentRoute.value.path).toBe('/settings')
    expect(router.currentRoute.value.query.category).toBe('diagnostics')
  })

  it('takes a device to the devices page', async () => {
    useDevicesStore().applySnapshot({
      identity: { deviceId: 'self', displayName: 'This device', platform: 'Windows', kernelVersion: '0.1.0' },
      devices: [{ deviceId: 'peer-1', displayName: 'Studio laptop', platform: 'macOS', online: true, paired: true }]
    } as never)

    const { router, wrapper } = mountPalette()
    await open(wrapper)

    await type('studio')
    const found = options()
    expect(found.length).toBe(1)
    found[0]!.click()
    await flushPromises()

    expect(router.currentRoute.value.path).toBe('/devices')
  })

  it('moves through results with the arrow keys and takes one with Enter', async () => {
    const { router, wrapper } = mountPalette()
    await open(wrapper)

    await type('i')
    // The first result is selected on arrival, so one ArrowDown reaches the second.
    const before = selected()
    expect(before.length).toBe(1)
    const firstName = before[0]!.textContent

    field().dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
    await flushPromises()
    const after = selected()
    expect(after.length).toBe(1)
    expect(after[0]!.textContent).not.toBe(firstName)

    field().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await flushPromises()
    expect(router.currentRoute.value.path).not.toBe('/')
  })

  it('paints hover and selection with different colours', async () => {
    // ★ A pointer resting on one row while the arrow keys move the selection to another used to paint both with
    // `bg-raised`, so the reader could not tell which one Enter would run (`09 §4`: hover, selected and focus must not
    // collapse into one highlight). The classes are read here rather than computed: this tree is not styled in jsdom.
    const { wrapper } = mountPalette()
    await open(wrapper)
    await type('i')

    const options = [...document.querySelectorAll<HTMLElement>('[role="option"]')]
    expect(options.length).toBeGreaterThan(1)
    const selectedRow = options.find(row => row.getAttribute('aria-selected') === 'true')
    expect(selectedRow, 'nothing is selected on arrival').toBeDefined()
    expect(selectedRow!.className).toContain('bg-raised')
    // The hover token is on the base class of every row, and it is not `raised`.
    for (const row of options)
      expect(row.className).toContain('hover:bg-surface')
  })

  it('does not take a slash typed into a field', async () => {
    mountPalette()
    await flushPromises()

    // A field somewhere on the page owns the keystroke. The box must stay shut.
    const elsewhere = document.createElement('input')
    document.body.append(elsewhere)
    elsewhere.dispatchEvent(new KeyboardEvent('keydown', { key: '/', bubbles: true }))
    await flushPromises()

    expect(document.querySelector('input[role="combobox"]')).toBeNull()
    expect(text()).not.toContain(messages.search.scope)
  })

  it('stays shut while another modal is open', async () => {
    mountPalette()
    await flushPromises()

    // ★ A visible antd modal owns the keyboard. The guard reads the DOM for `.ant-modal-root`/`.ant-modal-wrap` that
    // are not hidden by inline style — a modal drawn with a class or the `hidden` attribute would slip past it, so the
    // shape asserted here is the one the guard actually recognises.
    const root = document.createElement('div')
    root.className = 'ant-modal-root'
    const wrap = document.createElement('div')
    wrap.className = 'ant-modal-wrap'
    root.append(wrap)
    document.body.append(root)

    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: '/', bubbles: true }))
    await flushPromises()

    expect(document.querySelector('input[role="combobox"]')).toBeNull()
    expect(text()).not.toContain(messages.search.scope)
    root.remove()
  })

  it('opens on a slash pressed outside any field', async () => {
    mountPalette()
    await flushPromises()

    document.body.dispatchEvent(new KeyboardEvent('keydown', { key: '/', bubbles: true }))
    await flushPromises()

    expect(document.querySelector('input[role="combobox"]')).not.toBeNull()
    expect(text()).toContain(messages.search.scope)
  })

  it('shows what it searches only while the box is empty', async () => {
    const { wrapper } = mountPalette()
    await open(wrapper)

    expect(text()).toContain(messages.search.scope)

    await type('zzzz-nothing')
    expect(text()).not.toContain(messages.search.scope)
    expect(text()).toContain(messages.search.empty)
  })
})
