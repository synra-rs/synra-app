import { vi } from 'vitest'

interface TerminalOptions {
  disableStdin?: boolean
  screenReaderMode?: boolean
  linkHandler?: {
    allowNonHttpProtocols?: boolean
    activate: (event: MouseEvent, uri: string) => void
  }
}

interface TerminalDouble {
  options: TerminalOptions
  writes: string[]
  selection: string
  scrolls: number
  buffer: {
    active: {
      length: number
      viewportY: number
      baseY: number
    }
  }
}

const terminalState = vi.hoisted(() => ({ last: undefined as TerminalDouble | undefined }))
export { terminalState }
vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    options: TerminalOptions
    writes: string[] = []
    selection = ''
    scrolls = 0
    buffer = {
      active: { length: 0, viewportY: 0, baseY: 0 }
    }

    constructor(options: TerminalOptions) {
      this.options = options
      terminalState.last = this
    }

    loadAddon() {}
    open() {}
    write(value: string) { this.writes.push(value) }
    clear() { this.writes = [] }
    scrollToBottom() {
      this.scrolls += 1
      this.buffer.active.viewportY = this.buffer.active.baseY
    }

    getSelection() { return this.selection }
    dispose() {}
  }
}))
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { fit() {} } }))

export const DebugConsole = (await import('@/components/DebugConsole.vue')).default
