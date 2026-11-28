// @vitest-environment jsdom
import { flushPromises, mount } from '@vue/test-utils'
import { Select } from 'ant-design-vue'
import { expect, it, vi } from 'vitest'
import { publishWireLine } from '@/debug/wire-stream'
import { DebugConsole, terminalState } from './overlay-terminal.testkit'
import { messages } from './workflow.testkit'

it('returns focus to the wire bubble after the console closes', async () => {
  const debug = mount(DebugConsole, { attachTo: document.body })
  try {
    const bubble = [
      ...document.querySelectorAll<HTMLButtonElement>('button')
    ].find(button => button.getAttribute('aria-label') === messages.debug.open)!
    bubble.click()
    await flushPromises()
    const close = [
      ...document.querySelectorAll<HTMLButtonElement>('button')
    ].find(button => button.getAttribute('aria-label') === messages.debug.close)!
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
    const bubble = [
      ...document.querySelectorAll<HTMLButtonElement>('button')
    ].find(button => button.getAttribute('aria-label') === messages.debug.open)!
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
    const bubble = [
      ...document.querySelectorAll<HTMLButtonElement>('button')
    ].find(button => button.getAttribute('aria-label') === messages.debug.open)!
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
    publishWireLine({
      atMs: 1,
      direction: 'sent',
      peer: 'peer-a',
      label: 'event.a',
      bytes: 1
    })
    publishWireLine({
      atMs: 2,
      direction: 'received',
      peer: 'peer-b',
      label: 'event.b',
      bytes: 2
    })
    const bubble = [
      ...document.querySelectorAll<HTMLButtonElement>('button')
    ].find(button => button.getAttribute('aria-label') === messages.debug.open)!
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
    publishWireLine({
      atMs: 1,
      direction: 'sent',
      peer: 'peer-a',
      label: 'event.a',
      bytes: 1
    })
    const bubble = [
      ...document.querySelectorAll<HTMLButtonElement>('button')
    ].find(button => button.getAttribute('aria-label') === messages.debug.open)!
    bubble.click()
    await flushPromises()
    terminalState.last!.buffer.active.baseY = 4
    terminalState.last!.buffer.active.viewportY = 0
    publishWireLine({
      atMs: 2,
      direction: 'received',
      peer: 'peer-a',
      label: 'event.b',
      bytes: 2
    })
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
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText }
  })
  const debug = mount(DebugConsole, { attachTo: document.body })
  try {
    const bubble = [
      ...document.querySelectorAll<HTMLButtonElement>('button')
    ].find(button => button.getAttribute('aria-label') === messages.debug.open)!
    bubble.click()
    await flushPromises()
    terminalState.last!.selection = 'selected wire line'
    const summary = document.querySelector<HTMLElement>('.debug-tools-summary')!
    summary.click()
    const copy = [
      ...document.querySelectorAll<HTMLButtonElement>('.debug-tools-popover .debug-action')
    ].find(button => button.textContent?.trim() === messages.debug.copy)!
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
    const bubble = [
      ...document.querySelectorAll<HTMLButtonElement>('button')
    ].find(button => button.getAttribute('aria-label') === messages.debug.open)!
    publishWireLine({
      atMs: 1,
      direction: 'sent',
      peer: 'peer-1',
      label: '_synra.intent.request',
      bytes: 12,
      traceId: 'trace-1',
      requestId: 'request-1'
    })
    bubble.click()
    await flushPromises()
    const output = terminalState.last?.writes.join('') ?? ''
    const marker = 'synra-wire:'
    const start = output.indexOf(marker)
    const end = output.indexOf(String.fromCharCode(7), start)
    const link = start >= 0 && end >= 0
      ? output.slice(start + marker.length, end)
      : undefined
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
