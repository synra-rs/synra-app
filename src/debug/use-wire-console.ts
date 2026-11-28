import type { Ref } from 'vue'
import type { WireLine } from './wire'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { messages } from '../messages'
import { useDialogVisibility } from '../stores/dialogs'
import { useSettingsStore } from '../stores/settings'
import { useWireConsoleTerminal } from './use-wire-console-terminal'
import { formatWireLine, shortPeer } from './wire'
import { demoWireLines } from './wire-console-demo'
import { subscribeWireLines } from './wire-stream'

export function useWireConsole(host: Ref<HTMLDivElement | null>, bubble: Ref<HTMLButtonElement | null>, closeButton: Ref<HTMLButtonElement | null>, searchInput: Ref<HTMLInputElement | null>) {
  const open = ref(false)
  // Explicit diagnostics temporarily suspend workflows without hiding their own entry point.
  const visible = useDialogVisibility(open, 110, () => hide())
  const paused = ref(false)
  const filter = ref<'all' | 'sent' | 'received'>('all')
  const peerFilter = ref('')
  const selectedMetadata = ref<WireLine>()
  const unread = ref(0)
  const lineCount = ref(0)
  const sentCount = ref(0)
  const receivedCount = ref(0)
  const atLatest = ref(true)
  const pausedLines = ref(0)
  const settings = useSettingsStore()
  const enabled = computed(() => settings.value('ui.debugConsole') !== false)
  const history: WireLine[] = []
  const peers = ref<readonly string[]>([])
  const metadataIds = new WeakMap<object, string>()
  let nextMetadataId = 0
  const MAX_HISTORY = 8000
  let stopListening: (() => void) | null = null
  let resizeObserver: ResizeObserver | null = null
  let fitTimer: number | undefined
  let didInitialScroll = false
  const words = computed(() => messages.debug)
  const peerOptions = computed(() => [
    { label: words.value.peerAll, value: '' },
    ...peers.value.map(peer => ({
      label: shortPeer(peer),
      value: peer
    }))
  ])
  const {
    terminal,
    ensureTerminal,
    fit,
    isAtLatest,
    dispose,
    ...terminalControls
  } = useWireConsoleTerminal({
    host,
    closeButton,
    searchInput,
    visible,
    onMetadata: selectMetadata,
    onScroll(latest) {
      atLatest.value = latest
      if (latest)
        unread.value = 0
    },
    onReady: renderHistory
  })

  function hasMetadata(line: WireLine): boolean {
    return Boolean(line.traceId || line.requestId)
  }
  function metadataId(line: WireLine): string {
    const existing = metadataIds.get(line)
    if (existing)
      return existing
    const id = `line-${++nextMetadataId}`
    metadataIds.set(line, id)
    return id
  }
  function renderLine(line: WireLine): string {
    const rendered = formatWireLine(line)
    if (!hasMetadata(line))
      return rendered
    const id = metadataId(line)
    return `\x1B]8;;synra-wire:${id}\x07${rendered}\x1B]8;;\x07`
  }
  function selectMetadata(id: string): void {
    selectedMetadata.value = history.find(line => metadataId(line) === id)
  }
  function passes(line: WireLine): boolean {
    if (filter.value !== 'all' && line.direction !== filter.value)
      return false
    return !peerFilter.value || line.peer === peerFilter.value
  }
  function renderHistory(scroll = false): void {
    const term = terminal.value
    if (!term)
      return
    term.clear()
    lineCount.value = 0
    for (const line of history) {
      if (passes(line)) {
        term.write(`${renderLine(line)}\r\n`)
        lineCount.value += 1
      }
    }
    if (scroll)
      scrollToEnd()
  }
  function append(line: WireLine): void {
    const term = terminal.value
    if (!term || paused.value || !passes(line))
      return
    const wasAtLatest = isAtLatest()
    term.write(`${renderLine(line)}\r\n`)
    lineCount.value += 1
    atLatest.value = wasAtLatest
    if (open.value && !wasAtLatest)
      unread.value += 1
  }
  function observe(line: WireLine) {
    if (!enabled.value)
      return
    if (!peers.value.includes(line.peer))
      peers.value = [...peers.value, line.peer]
    history.push(line)
    if (history.length > MAX_HISTORY)
      history.shift()
    if (line.direction === 'sent')
      sentCount.value += 1
    else if (line.direction === 'received')
      receivedCount.value += 1
    if (paused.value)
      pausedLines.value += 1
    else
      append(line)
    if (!open.value) {
      unread.value += 1
    }
  }
  function scrollToEnd() {
    terminal.value?.scrollToBottom()
    atLatest.value = true
    unread.value = 0
  }
  function jumpToLatest(): void {
    scrollToEnd()
  }
  function setPaused(value: boolean): void {
    if (paused.value === value)
      return
    paused.value = value
    if (!value) {
      pausedLines.value = 0
      renderHistory(true)
    }
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
      if (!didInitialScroll) {
        didInitialScroll = true
        scrollToEnd()
      }
    }, 280)
  }
  function hide() {
    terminalControls.closeSearch(false)
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
    history.length = 0
    peers.value = []
    peerFilter.value = ''
    lineCount.value = 0
    sentCount.value = 0
    receivedCount.value = 0
    unread.value = 0
    pausedLines.value = 0
    atLatest.value = true
    selectedMetadata.value = undefined
  }
  function clearMetadata(): void {
    selectedMetadata.value = undefined
  }
  function onKey(event: KeyboardEvent) {
    if (!visible.value)
      return
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'f') {
      event.preventDefault()
      void terminalControls.openSearch()
      return
    }
    if (event.key === 'Escape') {
      event.preventDefault()
      if (terminalControls.searchOpen.value)
        terminalControls.closeSearch()
      else
        hide()
      return
    }
    if (event.key === 'Tab') {
      const controls = [
        ...(host.value?.closest('.debug-sheet')?.querySelectorAll<HTMLElement>('button, input, textarea, select, summary, [role="combobox"], [tabindex="0"]') ?? [])
      ]
        .filter(element => element.getClientRects().length > 0 && !element.hasAttribute('disabled'))
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
  function scheduleFit(): void {
    window.clearTimeout(fitTimer)
    fitTimer = window.setTimeout(() => void fit(), 48)
  }
  watch(filter, () => {
    if (!paused.value)
      renderHistory()
  })
  watch(peerFilter, () => {
    if (!paused.value)
      renderHistory()
  })
  onMounted(async () => {
    if (import.meta.env.DEV && new URLSearchParams(window.location.search).has('wire-demo')) {
      for (const line of demoWireLines(Date.now() - 12_000))
        observe(line)
      void show()
    }
    stopListening = subscribeWireLines(observe)
    if (typeof ResizeObserver !== 'undefined' && host.value) {
      resizeObserver = new ResizeObserver(scheduleFit)
      resizeObserver.observe(host.value)
    }
    window.addEventListener('keydown', onKey)
    window.addEventListener('resize', scheduleFit)
  })
  onBeforeUnmount(() => {
    stopListening?.()
    resizeObserver?.disconnect()
    resizeObserver = null
    window.removeEventListener('keydown', onKey)
    window.removeEventListener('resize', scheduleFit)
    window.clearTimeout(fitTimer)
    dispose()
  })
  return {
    open,
    visible,
    paused,
    pausedLines,
    atLatest,
    filter,
    peerFilter,
    peerOptions,
    unread,
    lineCount,
    sentCount,
    receivedCount,
    enabled,
    words,
    selectedMetadata,
    ...terminalControls,
    hide,
    toggle,
    clear,
    jumpToLatest,
    setPaused,
    clearMetadata
  }
}
