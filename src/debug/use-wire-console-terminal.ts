import type { FitAddon } from '@xterm/addon-fit'
import type { SearchAddon } from '@xterm/addon-search'
import type { Terminal } from '@xterm/xterm'
import type { Ref } from 'vue'
import { computed, nextTick, ref, watch } from 'vue'
import { messages } from '../messages'

interface WireTerminalOptions {
  host: Ref<HTMLDivElement | null>
  closeButton: Ref<HTMLButtonElement | null>
  searchInput: Ref<HTMLInputElement | null>
  visible: Readonly<Ref<boolean>>
  onMetadata: (id: string) => void
  onScroll: (latest: boolean) => void
  onReady: () => void
}

export function useWireConsoleTerminal({ host, closeButton, searchInput, visible, onMetadata, onScroll, onReady }: WireTerminalOptions) {
  const terminal = ref<Terminal | null>(null)
  let fitAddon: FitAddon | null = null
  let searchAddon: SearchAddon | null = null
  const searchOpen = ref(false)
  const searchQuery = ref('')
  const searchResult = ref<'idle' | 'found' | 'missing'>('idle')
  const copyState = ref<'idle' | 'copied' | 'failed'>('idle')
  let copyTimer: number | undefined

  function isAtLatest(): boolean {
    const term = terminal.value
    if (!term)
      return true
    const buffer = term.buffer.active
    return buffer.viewportY >= buffer.baseY
  }

  async function ensureTerminal() {
    if (terminal.value)
      return
    const [{ Terminal }, { FitAddon }, { SearchAddon }, _styles] = await Promise.all([
      import('@xterm/xterm'),
      import('@xterm/addon-fit'),
      import('@xterm/addon-search'),
      import('@xterm/xterm/css/xterm.css')
    ])
    await nextTick()
    if (!host.value)
      return
    const term = new Terminal({
      allowTransparency: false,
      convertEol: false,
      cursorBlink: false,
      disableStdin: true,
      screenReaderMode: true,
      linkHandler: {
        allowNonHttpProtocols: true,
        activate: (_event, uri) => {
          if (uri.startsWith('synra-wire:'))
            onMetadata(uri.slice('synra-wire:'.length))
        }
      },
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
    searchAddon = new SearchAddon()
    term.loadAddon(searchAddon)
    term.open(host.value)
    terminal.value = term
    if (typeof term.onScroll === 'function') {
      term.onScroll(() => {
        onScroll(isAtLatest())
      })
    }
    onReady()
  }
  async function fit() {
    if (!visible.value)
      return
    await nextTick()
    try {
      fitAddon?.fit()
    }
    catch {
    }
  }

  function runSearch(direction: 'next' | 'previous'): void {
    const query = searchQuery.value.trim()
    if (!query || !searchAddon) {
      searchResult.value = 'idle'
      searchAddon?.clearDecorations()
      return
    }
    const found = direction === 'next'
      ? searchAddon.findNext(query)
      : searchAddon.findPrevious(query)
    searchResult.value = found ? 'found' : 'missing'
  }
  async function openSearch(): Promise<void> {
    searchOpen.value = true
    await nextTick()
    searchInput.value?.focus()
    searchInput.value?.select()
    runSearch('next')
  }
  function closeSearch(restoreFocus = true): void {
    searchOpen.value = false
    searchQuery.value = ''
    searchResult.value = 'idle'
    searchAddon?.clearDecorations()
    if (restoreFocus)
      void nextTick(() => closeButton.value?.focus())
  }
  function onSearchKeydown(event: KeyboardEvent): void {
    event.stopPropagation()
    if (event.key === 'Escape') {
      event.preventDefault()
      closeSearch()
    }
    else if (event.key === 'Enter') {
      event.preventDefault()
      runSearch(event.shiftKey ? 'previous' : 'next')
    }
  }

  async function copy() {
    const term = terminal.value
    if (!term)
      return
    const selection = typeof term.getSelection === 'function'
      ? term.getSelection()
      : ''
    let text = selection
    if (text.length === 0) {
      const buffer = term.buffer.active
      const lines: string[] = []
      for (let y = 0; y < buffer.length; y += 1)
        lines.push(buffer.getLine(y)?.translateToString(true) ?? '')
      text = lines.join('\n').trimEnd()
    }
    try {
      await navigator.clipboard.writeText(text)
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
      return messages.debug.copied
    if (copyState.value === 'failed')
      return messages.debug.copyFailed
    return messages.debug.copy
  })

  watch(searchQuery, () => {
    if (searchOpen.value)
      runSearch('next')
  })

  function dispose(): void {
    window.clearTimeout(copyTimer)
    terminal.value?.dispose()
  }

  return {
    terminal,
    ensureTerminal,
    fit,
    isAtLatest,
    searchOpen,
    searchQuery,
    searchResult,
    openSearch,
    closeSearch,
    onSearchKeydown,
    searchPrevious: () => runSearch('previous'),
    searchNext: () => runSearch('next'),
    copy,
    copyLabel,
    dispose
  }
}
