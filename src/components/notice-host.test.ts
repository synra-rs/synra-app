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

const NoticeHost = (await import('./NoticeHost.vue')).default
const { useNoticesStore } = await import('../stores/notices')
const { messages } = await import('../messages')

function notice(message: string, params: Record<string, unknown>): KernelEvent {
  return { type: 'notice', payload: { message, params } } as KernelEvent
}

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('the notice host', () => {
  it('says what arrived and where it landed', async () => {
    useNoticesStore().apply(notice('notice.received', { name: 'notes.txt', path: 'C:/inbox/notes.txt' }))

    const wrapper = mount(NoticeHost)
    await flushPromises()

    expect(wrapper.text()).toContain(messages.notices.received('notes.txt'))
    expect(wrapper.text()).toContain(messages.notices.savedTo('C:/inbox/notes.txt'))
    wrapper.unmount()
  })

  it('replaces the previous notice rather than queueing a second banner', async () => {
    const store = useNoticesStore()
    store.apply(notice('notice.received', { name: 'first.txt', path: 'C:/inbox/first.txt' }))
    store.apply(notice('notice.received', { name: 'second.txt', path: 'C:/inbox/second.txt' }))

    const wrapper = mount(NoticeHost)
    await flushPromises()

    expect(wrapper.text()).toContain('second.txt')
    expect(wrapper.text()).not.toContain('first.txt')
    wrapper.unmount()
  })

  it('turns an unknown key into a sentence rather than printing it', async () => {
    useNoticesStore().apply(notice('notice.something-new', {}))

    const wrapper = mount(NoticeHost)
    await flushPromises()

    expect(wrapper.text()).toContain(messages.notices.generic)
    expect(wrapper.text()).not.toContain('notice.something-new')
    wrapper.unmount()
  })
})
