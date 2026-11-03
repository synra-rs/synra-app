import type { FitAddon } from '@xterm/addon-fit'
import type { Terminal } from '@xterm/xterm'
import type { Ref } from 'vue'
import type { WireLine } from './wire'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { messages } from '../messages'
import { useDialogVisibility } from '../stores/dialogs'
import { useSettingsStore } from '../stores/settings'
import { formatWireLine } from './wire'
import { subscribeWireLines } from './wire-stream'

export function useWireConsole(host: Ref<HTMLDivElement | null>, bubble: Ref<HTMLButtonElement | null>, closeButton: Ref<HTMLButtonElement | null>) {
  const open = ref(false)
  const visible = useDialogVisibility(open, 5)
  const paused = ref(false)
  const filter = ref<'all' | 'sent' | 'received'>('all')
  const unread = ref(0)
  const lineCount = ref(0)
  const sentCount = ref(0)
  const receivedCount = ref(0)
  const copyState = ref<'idle' | 'copied' | 'failed'>('idle')
  const settings = useSettingsStore()
  const enabled = computed(() => settings.value('ui.debugConsole') !== false)
  const terminal = ref<Terminal | null>(null)
  let fitAddon: FitAddon | null = null
  const pending: WireLine[] = []
  const MAX_PENDING = 2000
  let stopListening: (() => void) | null = null
  let copyTimer: number | undefined
  const words = computed(() => messages.debug)
  function playDemo() {
    const base = Date.now() - 12_000
    const lines: WireLine[] = [
      { atMs: base, direction: 'note', peer: 'local', label: 'scan started · 3000ms', bytes: 0 },
      { atMs: base + 40, direction: 'note', peer: 'lan', label: 'query', bytes: 0 },
      { atMs: base + 300, direction: 'note', peer: 'lan', label: 'heard 192.168.0.192:3413', bytes: 0 },
      { atMs: base + 320, direction: 'received', peer: '7f3a9c21', label: '_synra.device.changed', bytes: 214 },
      { atMs: base + 900, direction: 'note', peer: '7f3a9c21', label: 'dial 192.168.0.42:47100', bytes: 0 },
      { atMs: base + 980, direction: 'sent', peer: '7f3a9c21', label: '_synra.pair.request', bytes: 128 },
      { atMs: base + 1280, direction: 'received', peer: '7f3a9c21', label: '_synra.pair.response', bytes: 42 },
      { atMs: base + 1300, direction: 'note', peer: '7f3a9c21', label: 'paired · accepted', bytes: 0 },
      { atMs: base + 4100, direction: 'sent', peer: '7f3a9c21', label: '_synra.session.hello', bytes: 96 },
      { atMs: base + 4300, direction: 'received', peer: '7f3a9c21', label: '_synra.plugin.list', bytes: 2048 },
      { atMs: base + 5100, direction: 'sent', peer: '7f3a9c21', label: 'chunk #7', bytes: 65_536 },
      { atMs: base + 5120, direction: 'received', peer: '7f3a9c21', label: '_synra.transfer.accept', bytes: 64 }
    ]
    for (const line of lines)
      write(line)
  }
  function passes(line: WireLine): boolean {
    if (filter.value === 'all')
      return true
    return line.direction === filter.value
  }
  function write(line: WireLine) {
    const term = terminal.value
    if (!term) {
      if (pending.length >= MAX_PENDING)
        pending.shift()
      pending.push(line)
      return
    }
    if (paused.value || !passes(line))
      return
    term.write(`${formatWireLine(line)}\r\n`)
    lineCount.value += 1
  }
  function observe(line: WireLine) {
    if (!enabled.value)
      return
    if (line.direction === 'sent')
      sentCount.value += 1
    else if (line.direction === 'received')
      receivedCount.value += 1
    write(line)
    if (!open.value) {
      unread.value += 1
    }
  }
  async function ensureTerminal() {
    if (terminal.value)
      return
    const [{ Terminal }, { FitAddon }, _styles] = await Promise.all([
      import('@xterm/xterm'),
      import('@xterm/addon-fit'),
      import('@xterm/xterm/css/xterm.css')
    ])
    await nextTick()
    if (!host.value)
      return
    const term = new Terminal({
      allowTransparency: true,
      convertEol: false,
      cursorBlink: false,
      disableStdin: true,
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, "Cascadia Mono", monospace',
      fontSize: 13,
      lineHeight: 1.35,
      scrollback: 8000,
      theme: {
        background: '#080c12',
        foreground: '#dbe4f0',
        cursor: '#0b0f14',
        selectionBackground: 'rgba(120, 180, 255, 0.28)',
        scrollbarSliderBackground: 'rgba(255, 255, 255, 0.14)',
        scrollbarSliderHoverBackground: 'rgba(255, 255, 255, 0.28)',
        scrollbarSliderActiveBackground: 'rgba(255, 255, 255, 0.42)'
      }
    })
    fitAddon = new FitAddon()
    term.loadAddon(fitAddon)
    term.open(host.value)
    terminal.value = term
    for (const line of pending) {
      if (passes(line)) {
        term.write(`${formatWireLine(line)}\r\n`)
        lineCount.value += 1
      }
    }
    pending.length = 0
  }
  async function fit() {
    if (!open.value)
      return
    await nextTick()
    try {
      fitAddon?.fit()
    }
    catch {
    }
  }
  function scrollToEnd() {
    terminal.value?.scrollToBottom()
  }
  async function show() {
    open.value = true
    unread.value = 0
    await ensureTerminal()
    await fit()
    await nextTick()
    closeButton.value?.focus()
    window.setTimeout(() => {
      void fit()
      scrollToEnd()
    }, 280)
  }
  function hide() {
    open.value = false
    void nextTick(() => bubble.value?.focus())
  }
  watch(enabled, (on) => {
    if (!on)
      hide()
  })
  function toggle() {
    if (open.value)
      hide()
    else
      void show()
  }
  function clear() {
    terminal.value?.clear()
    pending.length = 0
    lineCount.value = 0
    sentCount.value = 0
    receivedCount.value = 0
    unread.value = 0
  }
  async function copy() {
    const term = terminal.value
    if (!term)
      return
    const buffer = term.buffer.active
    const lines: string[] = []
    for (let y = 0; y < buffer.length; y += 1)
      lines.push(buffer.getLine(y)?.translateToString(true) ?? '')
    try {
      await navigator.clipboard.writeText(lines.join('\n').trimEnd())
      copyState.value = 'copied'
    }
    catch {
      copyState.value = 'failed'
    }
    window.clearTimeout(copyTimer)
    copyTimer = window.setTimeout(() => {
      copyState.value = 'idle'
    }, 1500)
  }
  const copyLabel = computed(() => {
    if (copyState.value === 'copied')
      return words.value.copied
    if (copyState.value === 'failed')
      return words.value.copyFailed
    return words.value.copy
  })
  function onKey(event: KeyboardEvent) {
    if (!visible.value)
      return
    if (event.key === 'Escape') {
      event.preventDefault()
      hide()
    }
    if (event.key === 'Tab') {
      const controls = [...(host.value?.closest('.debug-sheet')?.querySelectorAll<HTMLElement>('button, textarea, [tabindex="0"]') ?? [])].filter(element => element.getClientRects().length > 0)
      const first = controls[0]
      const last = controls.at(-1)
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault()
        last?.focus()
      }
      else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault()
        first?.focus()
      }
    }
  }
  onMounted(async () => {
    if (import.meta.env.DEV && new URLSearchParams(window.location.search).has('wire-demo')) {
      playDemo()
      void show()
    }
    stopListening = subscribeWireLines(observe)
    window.addEventListener('keydown', onKey)
    window.addEventListener('resize', fit)
  })
  onBeforeUnmount(() => {
    stopListening?.()
    window.removeEventListener('keydown', onKey)
    window.removeEventListener('resize', fit)
    window.clearTimeout(copyTimer)
    terminal.value?.dispose()
  })
  return { open, visible, paused, filter, unread, lineCount, sentCount, receivedCount, enabled, words, hide, toggle, clear, copy, copyLabel }
}
