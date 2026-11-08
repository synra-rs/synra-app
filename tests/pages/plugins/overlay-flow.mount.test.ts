// @vitest-environment jsdom
import { DOMWrapper, flushPromises, mount } from '@vue/test-utils'
import { Drawer, Modal } from 'ant-design-vue'
import { expect, it, vi } from 'vitest'
import { answer, click, clickBody, messages, mountPage, PluginDetail } from './workflow.testkit'

vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    buffer = { active: { length: 0 } }
    loadAddon() {}
    open() {}
    write() {}
    clear() {}
    scrollToBottom() {}
    dispose() {}
  }
}))
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { fit() {} } }))

const PluginSync = (await import('@/components/plugins/plugin-sync.vue')).default
const DebugConsole = (await import('@/components/DebugConsole.vue')).default

async function openSync() {
  answer({ plugins_sync_plan: { pluginId: '@acme/reader', targets: [{ deviceId: 'android', displayName: 'Android', online: true, needsArtifact: true }] } })
  const page = await mountPage('/plugins?plugin=%40acme%2Freader')
  const detail = page.wrapper.getComponent(PluginDetail)
  await click(detail, messages.plugins.workflow.more)
  await clickBody(messages.plugins.syncAction, 'menuitem')
  const sync = page.wrapper.getComponent(PluginSync)
  return { ...page, detail, sync }
}

it('keeps sync outside the suspended detail and restores that same detail after close', async () => {
  const { detail, sync } = await openSync()
  expect(detail.getComponent(Drawer).props('open')).toBe(false)
  expect(sync.getComponent(Modal).props('open')).toBe(true)
  const panel = document.querySelector<HTMLElement>('.ant-modal')!
  expect(panel.closest('.ant-drawer')).toBeNull()
  expect(new DOMWrapper(panel).isVisible()).toBe(true)
  await click(sync, messages.plugins.sync.close)
  expect(detail.getComponent(Drawer).props('open')).toBe(true)
  await click(detail, messages.plugins.workflow.more)
  await clickBody(messages.plugins.syncAction, 'menuitem')
  expect(sync.getComponent(Modal).props('open')).toBe(true)
  expect(new DOMWrapper(document.querySelector('.ant-modal')!).isVisible()).toBe(true)
})

it('opens the console during sync and restores the pending workflow when it closes', async () => {
  const { sync } = await openSync()
  const debug = mount(DebugConsole, { attachTo: document.body })
  try {
    const bubble = [...document.querySelectorAll<HTMLButtonElement>('button')].find(button => button.getAttribute('aria-label') === messages.debug.open)!
    bubble.click()
    await flushPromises()
    const layer = document.querySelector('.debug-layer')!
    expect(layer.getAttribute('aria-hidden')).toBe('false')
    expect(sync.getComponent(Modal).props('open')).toBe(false)
    const close = [...layer.querySelectorAll<HTMLButtonElement>('button')].find(button => button.getAttribute('aria-label') === messages.debug.close)!
    close.click()
    await flushPromises()
    expect(sync.getComponent(Modal).props('open')).toBe(true)
    expect(new DOMWrapper(document.querySelector('.ant-modal')!).isVisible()).toBe(true)
  }
  finally {
    debug.unmount()
  }
})

it('releases a sync when its detail is removed and can open a different drawer afterwards', async () => {
  const { wrapper, router } = await openSync()
  await router.replace('/plugins')
  await flushPromises()
  expect(wrapper.findComponent(PluginSync).exists()).toBe(false)
  await click(wrapper, messages.plugins.publishers.manage(0))
  await vi.waitFor(() => {
    expect(document.querySelector('.ant-drawer-title')?.textContent).toContain(messages.plugins.publishers.title)
    expect(new DOMWrapper(document.querySelector('.ant-drawer-body')!).isVisible()).toBe(true)
  })
})
