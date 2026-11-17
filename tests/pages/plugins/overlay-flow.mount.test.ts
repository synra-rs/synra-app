// @vitest-environment jsdom
import { DOMWrapper, flushPromises, mount } from '@vue/test-utils'
import { Drawer, Modal, Select } from 'ant-design-vue'
import { expect, it, vi } from 'vitest'
import { publishWireLine } from '@/debug/wire-stream'
import { answer, click, clickBody, messages, mountPage, PluginDetail } from './workflow.testkit'

const terminalState = vi.hoisted(() => ({ last: undefined as { options: { disableStdin?: boolean, screenReaderMode?: boolean, linkHandler?: { allowNonHttpProtocols?: boolean, activate: (event: MouseEvent, uri: string) => void } }, writes: string[], selection: string, scrolls: number, getSelection: () => string, buffer: { active: { length: number, viewportY: number, baseY: number } } } | undefined }))
vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    options: { disableStdin?: boolean, screenReaderMode?: boolean, linkHandler?: { allowNonHttpProtocols?: boolean, activate: (event: MouseEvent, uri: string) => void } }
    writes: string[] = []
    selection = ''
    scrolls = 0
    buffer = { active: { length: 0, viewportY: 0, baseY: 0 } }
    constructor(options: { disableStdin?: boolean, screenReaderMode?: boolean, linkHandler?: { allowNonHttpProtocols?: boolean, activate: (event: MouseEvent, uri: string) => void } }) {
      this.options = options
      terminalState.last = this
    }

    loadAddon() {}
    open() {}
    write(value: string) { this.writes.push(value) }
    clear() { this.writes = [] }
    scrollToBottom() { this.scrolls += 1; this.buffer.active.viewportY = this.buffer.active.baseY }
    getSelection() { return this.selection }
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

it('returns focus to the wire bubble after the console closes', async () => {
  const debug = mount(DebugConsole, { attachTo: document.body })
  try {
    const bubble = [...document.querySelectorAll<HTMLButtonElement>('button')].find(button => button.getAttribute('aria-label') === messages.debug.open)!
    bubble.click()
    await flushPromises()
    const close = [...document.querySelectorAll<HTMLButtonElement>('button')].find(button => button.getAttribute('aria-label') === messages.debug.close)!
    close.click()
    await flushPromises()
    expect(document.activeElement).toBe(bubble)
  }
  finally {
    debug.unmount()
  }
})

it('opens the terminal search with Ctrl-F and closes it with Escape', async () => {
  const debug = mount(DebugConsole, { attachTo: document.body })
  try {
    const bubble = [...document.querySelectorAll<HTMLButtonElement>('button')].find(button => button.getAttribute('aria-label') === messages.debug.open)!
    bubble.click()
    await flushPromises()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true }))
    await flushPromises()
    const input = document.querySelector<HTMLInputElement>('.debug-search-input')
    expect(input, 'the terminal search field is missing').not.toBeNull()
    expect(input?.getAttribute('aria-label')).toBe(messages.debug.searchPlaceholder)
    input?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await flushPromises()
    expect(document.querySelector('.debug-search-input')).toBeNull()
  }
  finally {
    debug.unmount()
  }
})

it('keeps the wire terminal read-only and screen-reader accessible', async () => {
  const debug = mount(DebugConsole, { attachTo: document.body })
  try {
    const bubble = [...document.querySelectorAll<HTMLButtonElement>('button')].find(button => button.getAttribute('aria-label') === messages.debug.open)!
    bubble.click()
    await flushPromises()
    expect(terminalState.last?.options.disableStdin).toBe(true)
    expect(terminalState.last?.options.screenReaderMode).toBe(true)
  }
  finally {
    debug.unmount()
  }
})

it('filters the terminal view by peer without dropping history', async () => {
  const debug = mount(DebugConsole, { attachTo: document.body })
  try {
    publishWireLine({ atMs: 1, direction: 'sent', peer: 'peer-a', label: 'event.a', bytes: 1 })
    publishWireLine({ atMs: 2, direction: 'received', peer: 'peer-b', label: 'event.b', bytes: 2 })
    const bubble = [...document.querySelectorAll<HTMLButtonElement>('button')].find(button => button.getAttribute('aria-label') === messages.debug.open)!
    bubble.click()
    await flushPromises()
    expect(terminalState.last?.writes.join('')).toContain('event.a')
    expect(terminalState.last?.writes.join('')).toContain('event.b')

    const peerSelect = debug.getComponent(Select)
    await peerSelect.vm.$emit('update:value', 'peer-a')
    await flushPromises()
    expect(terminalState.last?.writes.join('')).toContain('event.a')
    expect(terminalState.last?.writes.join('')).not.toContain('event.b')

    await peerSelect.vm.$emit('update:value', '')
    await flushPromises()
    expect(terminalState.last?.writes.join('')).toContain('event.a')
    expect(terminalState.last?.writes.join('')).toContain('event.b')
  }
  finally {
    debug.unmount()
  }
})

it('keeps the terminal at history while reporting new lines', async () => {
  const debug = mount(DebugConsole, { attachTo: document.body })
  try {
    publishWireLine({ atMs: 1, direction: 'sent', peer: 'peer-a', label: 'event.a', bytes: 1 })
    const bubble = [...document.querySelectorAll<HTMLButtonElement>('button')].find(button => button.getAttribute('aria-label') === messages.debug.open)!
    bubble.click()
    await flushPromises()
    terminalState.last!.buffer.active.baseY = 4
    terminalState.last!.buffer.active.viewportY = 0
    publishWireLine({ atMs: 2, direction: 'received', peer: 'peer-a', label: 'event.b', bytes: 2 })
    await flushPromises()
    expect(document.querySelector('.debug-status')?.textContent).toContain(messages.debug.newLines(1))
    expect(terminalState.last?.scrolls).toBe(0)
  }
  finally {
    debug.unmount()
  }
})

it('copies selected terminal text when a selection exists', async () => {
  const writeText = vi.fn(async () => {})
  const previousClipboard = navigator.clipboard
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } })
  const debug = mount(DebugConsole, { attachTo: document.body })
  try {
    const bubble = [...document.querySelectorAll<HTMLButtonElement>('button')].find(button => button.getAttribute('aria-label') === messages.debug.open)!
    bubble.click()
    await flushPromises()
    terminalState.last!.selection = 'selected wire line'
    const summary = document.querySelector<HTMLElement>('.debug-tools-summary')!
    summary.click()
    const copy = [...document.querySelectorAll<HTMLButtonElement>('.debug-tools-popover .debug-action')].find(button => button.textContent?.trim() === messages.debug.copy)!
    copy.click()
    await flushPromises()
    expect(writeText).toHaveBeenCalledWith('selected wire line')
  }
  finally {
    debug.unmount()
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: previousClipboard })
  }
})

it('opens correlation details when a metadata line is clicked', async () => {
  const debug = mount(DebugConsole, { attachTo: document.body })
  try {
    const bubble = [...document.querySelectorAll<HTMLButtonElement>('button')].find(button => button.getAttribute('aria-label') === messages.debug.open)!
    publishWireLine({ atMs: 1, direction: 'sent', peer: 'peer-1', label: '_synra.intent.request', bytes: 12, traceId: 'trace-1', requestId: 'request-1' })
    bubble.click()
    await flushPromises()
    const output = terminalState.last?.writes.join('') ?? ''
    const marker = 'synra-wire:'
    const start = output.indexOf(marker)
    const end = output.indexOf(String.fromCharCode(7), start)
    const link = start >= 0 && end >= 0 ? output.slice(start + marker.length, end) : undefined
    expect(link, 'the metadata line is not linked').toBeDefined()
    terminalState.last?.options.linkHandler?.activate(new MouseEvent('click'), `synra-wire:${link}`)
    await flushPromises()
    expect(document.querySelector('.debug-metadata')?.textContent).toContain('trace-1')
    expect(document.querySelector('.debug-metadata')?.textContent).toContain('request-1')
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
  await wrapper.get('[data-testid="plugins-more"]').trigger('click')
  await clickBody(messages.plugins.publishers.manage(0), 'menuitem')
  await vi.waitFor(() => {
    expect(document.querySelector('.ant-drawer-title')?.textContent).toContain(messages.plugins.publishers.title)
    expect(new DOMWrapper(document.querySelector('.ant-drawer-body')!).isVisible()).toBe(true)
  })
})
