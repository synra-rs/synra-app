// @vitest-environment jsdom
/**
 * The page-less notice host.
 *
 * The event contract has carried `notice` since it was written and nothing drew it. The two assertions that matter: a
 * received file says its name and where it landed, and an unknown key becomes a sentence rather than printing the key.
 */

import type { KernelEvent } from '@synra/protocol'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'

const NoticeHost = (await import('@/components/NoticeHost.vue')).default
const { useNoticesStore } = await import('@/stores/notices')
const { messages } = await import('@/messages')

function notice(message: string, params: Record<string, unknown>): KernelEvent {
  return { type: 'notice', payload: { message, params } } as KernelEvent
}

/** The host reads the router for its one action, so every mount needs one. */
function mountHost() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/plugins', component: { template: '<div />' } },
      { path: '/:pathMatch(.*)*', component: { template: '<div />' } }
    ]
  })
  return { router, wrapper: mount(NoticeHost, { attachTo: document.body, global: { plugins: [router] } }) }
}

function bodyText(): string {
  return document.body.textContent ?? ''
}

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('the notice host', () => {
  it('says what arrived and where it landed', async () => {
    useNoticesStore().apply(notice('notice.received', { name: 'notes.txt', path: 'C:/inbox/notes.txt' }))

    const { wrapper } = mountHost()
    await flushPromises()

    expect(bodyText()).toContain(messages.notices.received('notes.txt'))
    expect(bodyText()).toContain(messages.notices.savedTo('C:/inbox/notes.txt'))
    wrapper.unmount()
  })

  it('keeps a bounded stack of recent notices', async () => {
    const store = useNoticesStore()
    store.apply(notice('notice.received', { name: 'first.txt', path: 'C:/inbox/first.txt' }))
    store.apply(notice('notice.received', { name: 'second.txt', path: 'C:/inbox/second.txt' }))
    store.apply(notice('notice.received', { name: 'third.txt', path: 'C:/inbox/third.txt' }))
    store.apply(notice('notice.received', { name: 'fourth.txt', path: 'C:/inbox/fourth.txt' }))

    const { wrapper } = mountHost()
    await flushPromises()

    expect(bodyText()).toContain('fourth.txt')
    expect(bodyText()).toContain('third.txt')
    expect(bodyText()).toContain('second.txt')
    expect(bodyText()).not.toContain('first.txt')
    expect(bodyText()).toContain(messages.notices.more(1))
    wrapper.unmount()
  })

  it('says when a package opened from disk has installed', async () => {
    useNoticesStore().apply(notice('notice.installed', {
      pluginId: '@acme/hello',
      version: '1.2.0',
      warnings: ['The package is not signed.']
    }))

    const { wrapper } = mountHost()
    await flushPromises()

    expect(bodyText()).toContain(messages.notices.installed('@acme/hello', '1.2.0'))
    // ★ The verifier's warning is the part a file opened from the desktop most needs: it is not signed.
    expect(bodyText()).toContain('The package is not signed.')
    wrapper.unmount()
  })

  it('says when a package replaced an older version', async () => {
    useNoticesStore().apply(notice('notice.updated', {
      pluginId: '@acme/hello',
      version: '2.0.0',
      warnings: ['This update asks for a new capability.']
    }))

    const { wrapper } = mountHost()
    await flushPromises()

    expect(bodyText()).toContain(messages.notices.updated('@acme/hello', '2.0.0'))
    expect(bodyText()).toContain('This update asks for a new capability.')
    wrapper.unmount()
  })

  it('says how many updates the daily check found', async () => {
    // ★ One banner for the pass, not one per plugin: the next step is the same for all of them, and a stack of identical
    // notices is what a person learns to close without reading.
    useNoticesStore().apply(notice('notice.updates', { count: 2, plugins: ['@acme/one', '@acme/two'] }))

    const { wrapper } = mountHost()
    await flushPromises()

    expect(bodyText()).toContain('2 plugin updates are available.')
    expect(bodyText()).toContain('@acme/one, @acme/two')
    expect(bodyText()).toContain(messages.notices.review)
    wrapper.unmount()
  })

  it('says when a package is newer than the one on disk', async () => {
    useNoticesStore().apply(notice('notice.newerInstalled', {
      pluginId: '@acme/hello',
      installed: '2.0.0',
      offered: '1.0.0'
    }))

    const { wrapper } = mountHost()
    await flushPromises()

    expect(bodyText()).toContain(messages.notices.newerInstalled('@acme/hello', '2.0.0', '1.0.0'))
    wrapper.unmount()
  })

  it('shows the reason an invalid version prevented opening a package', async () => {
    const pluginId = '@acme/hello'
    const reason = 'Invalid prerelease identifier in 1.0.0-01'
    useNoticesStore().apply(notice('notice.invalidPluginVersion', { pluginId, reason }))
    const { wrapper } = mountHost()
    await flushPromises()

    expect(bodyText()).toContain(messages.notices.invalidPluginVersion(pluginId))
    expect(bodyText()).toContain(reason)
    wrapper.unmount()
  })

  it('takes a package notice to the plugins page', async () => {
    // ★ The notice says what landed; the next step is on another page. A sentence with no way there is half an answer.
    useNoticesStore().apply(notice('notice.installed', { pluginId: '@acme/hello', version: '1.2.0' }))
    const { router, wrapper } = mountHost()
    await flushPromises()

    const action = [...document.body.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === messages.notices.review)
    expect(action, 'the package notice has no way to the plugins page').toBeDefined()
    action?.click()
    await flushPromises()

    expect(router.currentRoute.value.path).toBe('/plugins')
    expect(router.currentRoute.value.query.plugin).toBe('@acme/hello')
    wrapper.unmount()
  })

  it('says when a drop had nothing it could do', async () => {
    useNoticesStore().apply(notice('notice.dropIgnored', {}))

    const { wrapper } = mountHost()
    await flushPromises()

    expect(bodyText()).toContain(messages.notices.dropIgnored)
    wrapper.unmount()
  })

  it('says when a synra link is not one this build understands', async () => {
    // ★ The deep-link handler focuses the window for any `synra://` link; doing that silently for an unknown one would
    // look like it worked. The notice is the sentence that says it did not.
    useNoticesStore().apply(notice('notice.unknownLink', { url: 'synra://settings' }))

    const { wrapper } = mountHost()
    await flushPromises()

    expect(bodyText()).toContain(messages.notices.unknownLink('synra://settings'))
    expect(bodyText()).not.toContain('notice.unknownLink')
    wrapper.unmount()
  })

  it('turns an unknown key into a sentence rather than printing it', async () => {
    useNoticesStore().apply(notice('notice.something-new', {}))

    const { wrapper } = mountHost()
    await flushPromises()

    expect(bodyText()).toContain(messages.notices.generic)
    expect(bodyText()).not.toContain('notice.something-new')
    wrapper.unmount()
  })
})
