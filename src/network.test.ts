// @vitest-environment jsdom
import { mount } from '@vue/test-utils'
import { describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick } from 'vue'

import { useNetworkStatus } from './network'

/** The smallest component that reads the composable; the point is the listener, not a page. */
const Probe = defineComponent({
  setup() {
    const online = useNetworkStatus()
    return () => h('span', online.value ? 'online' : 'offline')
  }
})

describe('the network status', () => {
  it('follows the browser flag and takes its listeners with it', async () => {
    // ★ `navigator.onLine` is what the platform believes, which is the question "should sharing work right now" — not
    // "can this peer be reached". A page that bound a socket to answer it would be solving a different problem.
    const original = Object.getOwnPropertyDescriptor(navigator, 'onLine')
    Object.defineProperty(navigator, 'onLine', { configurable: true, value: false })
    const removed = vi.spyOn(window, 'removeEventListener')

    const wrapper = mount(Probe)
    await nextTick()
    expect(wrapper.text()).toBe('offline')

    Object.defineProperty(navigator, 'onLine', { configurable: true, value: true })
    window.dispatchEvent(new Event('online'))
    await nextTick()
    expect(wrapper.text()).toBe('online')

    wrapper.unmount()
    expect(removed).toHaveBeenCalledWith('online', expect.any(Function))
    expect(removed).toHaveBeenCalledWith('offline', expect.any(Function))

    removed.mockRestore()
    if (original)
      Object.defineProperty(navigator, 'onLine', original)
  })
})
