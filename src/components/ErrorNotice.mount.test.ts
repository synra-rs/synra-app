// @vitest-environment jsdom
/**
 * `ErrorNotice` — the one place every page draws a kernel refusal.
 *
 * # What is asserted here that page tests cannot
 *
 * The notice turns some suggested actions into buttons. Where a button goes is decided in one function, `to()`, and a
 * page test only ever sees the outcome through its own screen. Two things are checked at this level:
 *
 * 1. **`installPlugin` keeps the plugin id.** `PluginsPage` reads `?plugin=` to open that row; without it the button
 *    drops the reader at the top of the list and says "find it yourself".
 * 2. **An action with no screen is words, not a dead button.** `requestCapability` has nothing to open, and a button
 *    that does nothing is worse than a sentence.
 */
import type { SuggestedAction } from '@synra/protocol'
import { flushPromises, mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'

import ErrorNotice from './ErrorNotice.vue'

/** A router with the four real paths, so `router.push` resolves instead of warning. */
function router() {
  return createRouter({
    history: createMemoryHistory(),
    routes: ['/', '/devices', '/plugins', '/settings'].map(path => ({ path, component: { template: '<div />' } }))
  })
}

async function mountNotice(actions: readonly SuggestedAction[]) {
  const instance = router()
  await instance.push('/')
  const wrapper = mount(ErrorNotice, {
    props: { code: 'unavailable', actions },
    global: { plugins: [instance] }
  })
  await flushPromises()
  return { wrapper, instance }
}

describe('error notice actions', () => {
  it('keeps the plugin id when sending the reader to plugins', async () => {
    // ★ The kernel names one plugin. `PluginsPage` opens the row for `?plugin=`; without the id this button means
    // "go look through the list".
    const { wrapper, instance } = await mountNotice([{ kind: 'installPlugin', pluginId: '@acme/hello' }])
    const button = wrapper.findAll('button').find(candidate => candidate.text() === 'Open plugins')
    expect(button, 'the plugins button is missing').toBeDefined()

    await button!.trigger('click')
    await flushPromises()
    expect(instance.currentRoute.value.path).toBe('/plugins')
    expect(instance.currentRoute.value.query.plugin).toBe('@acme/hello')
    wrapper.unmount()
  })

  it('encodes a plugin id that would otherwise break the query', async () => {
    // Scope-qualified ids are the normal case (`@acme/hello`), and `/` inside a query value would be read as a path.
    const { wrapper, instance } = await mountNotice([{ kind: 'installPlugin', pluginId: 'a b&c' }])
    const button = wrapper.findAll('button').find(candidate => candidate.text() === 'Open plugins')!
    await button.trigger('click')
    await flushPromises()
    expect(instance.currentRoute.value.query.plugin).toBe('a b&c')
    wrapper.unmount()
  })

  it('keeps the settings section when sending the reader to settings', async () => {
    // ★ `SettingsPage` opens the column for `?category=` and ignores a value it does not know. Dropping the section sent
    // every "open settings" to the first column, even when the kernel named a different one.
    const { wrapper, instance } = await mountNotice([{ kind: 'openSettings', section: 'security' }])
    const button = wrapper.findAll('button').find(candidate => candidate.text() === 'Open settings')
    expect(button, 'the settings button is missing').toBeDefined()

    await button!.trigger('click')
    await flushPromises()
    expect(instance.currentRoute.value.path).toBe('/settings')
    expect(instance.currentRoute.value.query.category).toBe('security')
    wrapper.unmount()
  })

  it('renders a capability request as a direction, not a button', async () => {
    // There is no screen for "grant this capability", so the notice must not offer a control that does nothing.
    const { wrapper } = await mountNotice([{ kind: 'requestCapability', capability: 'network:fetch:api.example.com' }])
    const texts = wrapper.findAll('button').map(button => button.text())
    expect(texts).not.toContain('Ask for the permission')
    expect(wrapper.text()).toContain('Ask for the permission')
    wrapper.unmount()
  })

  it('shows no action row at all when the kernel sends none', async () => {
    const { wrapper } = await mountNotice([])
    expect(wrapper.findAll('button')).toHaveLength(0)
    wrapper.unmount()
  })
})
