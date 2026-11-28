// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import App from '@/App.vue'
import { IDENTITY } from '@/ipc/fake-kernel-fixtures'
import { messages } from '@/messages'
import { useDevicesStore } from '@/stores/devices'

const native = vi.hoisted(() => ({
  call: vi.fn(),
  listenBack: vi.fn(),
  exit: vi.fn(),
  window: vi.fn(),
  close: vi.fn(),
  onResized: vi.fn(),
  stopResize: vi.fn(),
  isMaximized: vi.fn()
}))
vi.mock('@/app/kernel', () => ({
  kernel: { call: native.call }
}))
vi.mock('@tauri-apps/api/app', () => ({ onBackButtonPress: native.listenBack, exit: native.exit }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: native.window }))

function media(widthRem: number) {
  function query(breakpoint: number) {
    const listeners = new Set<() => void>()
    return {
      matches: widthRem >= breakpoint,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn((_event: string, listener: () => void) => listeners.add(listener)),
      removeEventListener: vi.fn((_event: string, listener: () => void) => listeners.delete(listener)),
      resize(rem: number) {
        const next = rem >= breakpoint
        if (this.matches !== next) {
          this.matches = next
          listeners.forEach(listener => listener())
        }
      }
    }
  }
  const sidebar = query(48)
  const wide = query(64)
  vi.stubGlobal('matchMedia', vi.fn((selector: string) => {
    if (selector === '(min-width: 48rem)')
      return sidebar
    if (selector === '(min-width: 64rem)')
      return wide
    return {
      matches: false,
      addListener: vi.fn(),
      removeListener: vi.fn()
    }
  }))
  return {
    resize(rem: number) {
      sidebar.resize(rem)
      wide.resize(rem)
    },
    expectDisposed() {
      for (const breakpoint of [sidebar, wide]) {
        expect(breakpoint.addEventListener).toHaveBeenCalledOnce()
        expect(breakpoint.removeEventListener.mock.calls).toEqual(breakpoint.addEventListener.mock.calls)
      }
    }
  }
}

function touchNavigator(userAgent: string): void {
  vi.stubGlobal('navigator', {
    languages: navigator.languages,
    language: navigator.language,
    userAgent,
    maxTouchPoints: 5
  })
}

async function render(platform: string, nativeShell: boolean) {
  vi.stubGlobal('__TAURI_INTERNALS__', nativeShell
    ? {}
    : undefined)
  native.call.mockImplementation(async (command: string) => {
    if (command === 'devices_snapshot') {
      return {
        ok: true,
        value: {
          identity: { ...IDENTITY, platform },
          devices: []
        }
      }
    }
    if (command === 'config_get') {
      return {
        ok: true,
        value: { values: {}, overridden: [] }
      }
    }
    throw new Error(`unexpected ${command}`)
  })
  const pinia = createPinia()
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      {
        path: '/',
        component: { template: '<div />' }
      }
    ]
  })
  await router.push('/')
  const wrapper = mount(App, {
    attachTo: document.body,
    global: {
      plugins: [pinia, router],
      stubs: {
        CrashNotice: true,
        NoticeHost: true,
        TransferPrompt: true,
        PairingPrompt: true,
        InputPrompt: true,
        DebugConsole: true,
        SearchPalette: true
      }
    }
  })
  return {
    wrapper,
    router,
    devices: useDevicesStore(pinia)
  }
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  native.isMaximized.mockResolvedValue(false)
  native.close.mockResolvedValue(undefined)
  native.onResized.mockResolvedValue(native.stopResize)
  native.window.mockReturnValue({
    close: native.close,
    isMaximized: native.isMaximized,
    onResized: native.onResized
  })
})
afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('viewport and native host', () => {
  it('keeps desktop controls on a touch Windows host across layout changes', async () => {
    touchNavigator('Windows Touch')
    const viewport = media(80)
    const { wrapper } = await render('windows', true)
    try {
      await flushPromises()
      const screen = wrapper.get('.screen')
      expect(screen.attributes('data-navigation')).toBe('sidebar')
      expect(screen.classes()).not.toContain('mobile-host')
      viewport.resize(56)
      await nextTick()
      expect(screen.attributes('data-navigation')).toBe('rail')
      viewport.resize(40)
      await nextTick()
      expect(screen.attributes('data-navigation')).toBe('bottom')
      await wrapper.get(`button[aria-label="${messages.window.close}"]`).trigger('click')
      expect(native.close).toHaveBeenCalledOnce()
      expect(native.listenBack).not.toHaveBeenCalled()
    }
    finally {
      wrapper.unmount()
    }
    await flushPromises()
    viewport.expectDisposed()
    expect(native.stopResize).toHaveBeenCalledOnce()
  })

  it('uses the compact layout in mobile browser preview without native actions', async () => {
    touchNavigator('Android Mobile')
    const viewport = media(30)
    const { wrapper } = await render('android', false)
    try {
      await flushPromises()
      expect(wrapper.get('.screen').attributes('data-navigation')).toBe('bottom')
      expect(native.window).not.toHaveBeenCalled()
      expect(native.listenBack).not.toHaveBeenCalled()
    }
    finally {
      wrapper.unmount()
    }
    viewport.expectDisposed()
  })

  it('retains one Android system listener across a wide viewport and disposes late registration', async () => {
    const viewport = media(35)
    const unregister = vi.fn()
    let complete: (listener: { unregister: () => void }) => void = () => {}
    native.listenBack.mockImplementation(() => new Promise<{ unregister: () => void }>((resolve) => {
      complete = resolve
    }))
    const { wrapper, devices } = await render('android', true)
    try {
      await flushPromises()
      expect(wrapper.get('.screen').classes()).toContain('mobile-host')
      expect(devices.identity?.platform).toBe('android')
      expect(native.call).toHaveBeenCalledWith('config_get')
      expect(native.listenBack).toHaveBeenCalledOnce()
      viewport.resize(72)
      await nextTick()
      expect(wrapper.get('.screen').attributes('data-navigation')).toBe('sidebar')
      expect(native.listenBack).toHaveBeenCalledOnce()
      expect(native.window).not.toHaveBeenCalled()
    }
    finally {
      wrapper.unmount()
    }
    complete({ unregister })
    await flushPromises()
    expect(unregister).toHaveBeenCalledOnce()
    viewport.expectDisposed()
  })

  it('uses the registered Android event to leave a direct link in a wide layout', async () => {
    media(72)
    native.listenBack.mockResolvedValue({
      unregister: vi.fn()
    })
    const { wrapper, router } = await render('android', true)
    try {
      await flushPromises()
      await router.replace('/?plugin=demo#details')
      const handle = native.listenBack.mock.calls[0]?.[0] as (event: { canGoBack: boolean }) => void
      handle({ canGoBack: false })
      await flushPromises()
      expect(router.currentRoute.value.fullPath).toBe('/')
      expect(native.exit).not.toHaveBeenCalled()
    }
    finally {
      wrapper.unmount()
    }
  })

  it('loads settings and devices when Android native listener registration fails', async () => {
    media(72)
    const error = new Error('registration rejected')
    const diagnostic = vi.spyOn(console, 'warn').mockImplementation(() => {})
    native.listenBack.mockRejectedValue(error)
    const { wrapper, devices } = await render('android', true)
    try {
      await flushPromises()
      expect(devices.identity?.platform).toBe('android')
      expect(native.call).toHaveBeenCalledWith('config_get')
      expect(diagnostic).toHaveBeenCalledWith('Synra Android back', error)
    }
    finally {
      wrapper.unmount()
    }
  })
})
