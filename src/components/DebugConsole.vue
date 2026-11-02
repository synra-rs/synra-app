<script setup lang="ts">
/**
 * The wire console: a floating bubble on every page, and a bottom drawer that shows the kernel's traffic.
 *
 * # Why a terminal rather than a list
 *
 * The thing being read is a stream of protocol frames: alignment, colour and monospace are what make one frame's shape
 * legible at a glance. `@xterm/xterm` is the same renderer the reference implementation (antfu/node-modules-inspector)
 * and `peon-hall` use, so ANSI colour and scrollback behave the way a reader already expects.
 *
 * # Why the bubble is global
 *
 * Pairing, discovery and transfer failures are not one page's problem. A console mounted inside the devices screen would
 * be absent exactly when a user is looking at settings and wondering why nothing arrives.
 *
 * # Why events are buffered before the drawer opens
 *
 * The failure a user opens the console to see usually happened a moment earlier. Dropping everything until the click
 * would make the console show a healthy silence. The buffer is bounded and cleared when the terminal takes it.
 */
import type { FitAddon } from '@xterm/addon-fit'
import type { Terminal } from '@xterm/xterm'
import type { SequencedKernelEvent } from '@synra/protocol'
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'

import { kernel } from '../kernel'
import { messages } from '../messages'
import type { WireLine } from '../debug/wire'
import { formatWireLine } from '../debug/wire'

const open = ref(false)
const paused = ref(false)
const filter = ref<'all' | 'sent' | 'received'>('all')
const unread = ref(0)
const lineCount = ref(0)
const sentCount = ref(0)
const receivedCount = ref(0)
const pulse = ref(false)
const copyState = ref<'idle' | 'copied' | 'failed'>('idle')

const host = ref<HTMLDivElement | null>(null)
const terminal = ref<Terminal | null>(null)
/** Loaded with the terminal, because xterm is a third of the app's JavaScript and only this drawer uses it. */
let fitAddon: FitAddon | null = null
/** Lines heard while the terminal did not exist yet; flushed on mount. */
const pending: WireLine[] = []
const MAX_PENDING = 2000
let stopListening: (() => void) | null = null
let pulseTimer: number | undefined
let copyTimer: number | undefined

const words = computed(() => messages.debug)

/**
 * A handful of frames for `?wire-demo=1`, so the drawer can be reviewed without a peer.
 *
 * `import.meta.env.DEV` is replaced at build time, so this branch and its lines are absent from a packaged window.
 */
function playDemo() {
  const base = Date.now() - 12_000
  const lines: WireLine[] = [
    { atMs: base, direction: 'note', peer: 'local', label: 'scan started · 3000ms', bytes: 0 },
    { atMs: base + 320, direction: 'received', peer: '7f3a9c21', label: '_synra.device.changed', bytes: 214 },
    { atMs: base + 900, direction: 'note', peer: '7f3a9c21', label: 'dial 192.168.0.42:47100', bytes: 0 },
    { atMs: base + 980, direction: 'sent', peer: '7f3a9c21', label: '_synra.pair.request', bytes: 128 },
    { atMs: base + 1280, direction: 'received', peer: '7f3a9c21', label: '_synra.pair.response', bytes: 42 },
    { atMs: base + 1300, direction: 'note', peer: '7f3a9c21', label: 'paired · accepted', bytes: 0 },
    { atMs: base + 4100, direction: 'sent', peer: '7f3a9c21', label: '_synra.session.hello', bytes: 96 },
    { atMs: base + 4300, direction: 'received', peer: '7f3a9c21', label: '_synra.plugin.list', bytes: 2048 },
    { atMs: base + 5100, direction: 'sent', peer: '7f3a9c21', label: 'chunk #7', bytes: 65_536 },
    { atMs: base + 5120, direction: 'received', peer: '7f3a9c21', label: '_synra.transfer.accept', bytes: 64 },
  ]
  for (const line of lines)
    write(line)
}

/** Whether a line passes the header's filter. */
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

function observe(payload: unknown) {
  const event = payload as SequencedKernelEvent | undefined
  if (!event || event.type !== 'debugWire')
    return
  const line = event.payload as WireLine
  if (line.direction === 'sent')
    sentCount.value += 1
  else if (line.direction === 'received')
    receivedCount.value += 1
  write(line)
  if (!open.value) {
    unread.value += 1
    pulse.value = true
    window.clearTimeout(pulseTimer)
    pulseTimer = window.setTimeout(() => {
      pulse.value = false
    }, 900)
  }
}

/** Loads xterm on the first open, then opens it into the now-laid-out host. */
async function ensureTerminal() {
  if (terminal.value)
    return
  const [{ Terminal }, { FitAddon }, _styles] = await Promise.all([
    import('@xterm/xterm'),
    import('@xterm/addon-fit'),
    import('@xterm/xterm/css/xterm.css'),
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
    fontSize: 12,
    lineHeight: 1.35,
    scrollback: 8000,
    theme: {
      background: '#080c12',
      foreground: '#dbe4f0',
      cursor: '#0b0f14',
      selectionBackground: 'rgba(120, 180, 255, 0.28)',
      scrollbarSliderBackground: 'rgba(255, 255, 255, 0.14)',
      scrollbarSliderHoverBackground: 'rgba(255, 255, 255, 0.28)',
      scrollbarSliderActiveBackground: 'rgba(255, 255, 255, 0.42)',
    },
  })
  fitAddon = new FitAddon()
  term.loadAddon(fitAddon)
  term.open(host.value)
  terminal.value = term
  // What arrived before the drawer existed is still worth reading.
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
  } catch {
    // The drawer is mid-animation; the transitionend below measures again.
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
  // The panel animates for 260ms; measuring only before it ends computes columns from a half-open height.
  window.setTimeout(() => {
    void fit()
    scrollToEnd()
  }, 280)
}

function hide() {
  open.value = false
}

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
  } catch {
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
  if (event.key === 'Escape' && open.value)
    hide()
}

onMounted(async () => {
  if (import.meta.env.DEV && new URLSearchParams(window.location.search).has('wire-demo')) {
    playDemo()
    void show()
  }

  // A shell always has this; a mount test's kernel double may not, and a console that throws would take the page with it.
  if (typeof kernel.subscribe === 'function')
    stopListening = await kernel.subscribe('kernel-event', observe)
  window.addEventListener('keydown', onKey)
  window.addEventListener('resize', fit)
})

onBeforeUnmount(() => {
  stopListening?.()
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('resize', fit)
  window.clearTimeout(pulseTimer)
  window.clearTimeout(copyTimer)
  terminal.value?.dispose()
})
</script>

<template>
  <!--
    Teleported to the body: the shell is a CSS grid with `overflow-hidden`, and a fixed bubble inside a grid cell is
    clipped by the cell's scroll container in some engines. The console belongs to the window, not to a page's pane.
  -->
  <Teleport to="body">
    <button
      v-show="!open"
      type="button"
      class="debug-bubble border-0 bg-transparent"
      :class="{ 'debug-bubble-pulse': pulse }"
      :aria-label="words.open"
      :title="words.open"
      @click="toggle"
    >
      <span class="debug-bubble-glow" aria-hidden="true" />
      <svg viewBox="0 0 24 24" class="debug-bubble-icon" aria-hidden="true">
        <path
          d="M4 6.5 9 11l-5 4.5M12 18h8"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
      <span v-if="unread > 0" class="debug-bubble-badge">{{ unread > 99 ? '99+' : unread }}</span>
    </button>

    <div class="debug-layer" :class="{ 'debug-layer-open': open }" :aria-hidden="!open">
      <div class="debug-scrim" @click="hide" />

      <section class="debug-sheet" role="dialog" :aria-label="words.title">
        <div class="debug-grip" aria-hidden="true" />

        <header class="debug-head">
          <div class="debug-heading">
            <div class="debug-title">
              {{ words.title }}
              <span class="debug-scheme">synra://wire</span>
            </div>
          </div>

          <div class="debug-counters" aria-hidden="true">
            <span class="debug-counter debug-counter-sent">↑ {{ sentCount }}</span>
            <span class="debug-counter debug-counter-received">↓ {{ receivedCount }}</span>
            <span class="debug-counter">{{ words.lines(lineCount) }}</span>
          </div>

          <div class="debug-actions">
            <button type="button" class="debug-action border-0 bg-transparent" @click="paused = !paused">
              {{ paused ? words.resume : words.pause }}
            </button>
            <button type="button" class="debug-action border-0 bg-transparent" @click="clear">{{ words.clear }}</button>
            <button type="button" class="debug-action border-0 bg-transparent" @click="copy">{{ copyLabel }}</button>
            <button type="button" class="debug-action debug-action-close border-0 bg-transparent" :aria-label="words.close" @click="hide">
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="m4 4 8 8M12 4l-8 8" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
              </svg>
            </button>
          </div>
        </header>

        <div class="debug-filters">
          <button
            type="button"
            class="debug-filter border-0 bg-transparent"
            :class="{ 'debug-filter-on': filter === 'all' }"
            @click="filter = 'all'"
          >
            {{ words.all }}
          </button>
          <button
            type="button"
            class="debug-filter debug-filter-sent border-0 bg-transparent"
            :class="{ 'debug-filter-on': filter === 'sent' }"
            @click="filter = 'sent'"
          >
            {{ words.sent }}
          </button>
          <button
            type="button"
            class="debug-filter debug-filter-received border-0 bg-transparent"
            :class="{ 'debug-filter-on': filter === 'received' }"
            @click="filter = 'received'"
          >
            {{ words.received }}
          </button>
          <span class="debug-hint">{{ words.hintOpen }}</span>
        </div>

        <div class="debug-term-wrap">
          <div ref="host" class="debug-term" />
          <div v-if="lineCount === 0" class="debug-empty">{{ words.empty }}</div>
        </div>
      </section>
    </div>
  </Teleport>
</template>

<style scoped>
/*
 * The bubble is deliberately the only saturated thing on the page: it is the entry to a diagnostic tool, and a reader
 * who has forgotten it exists should find it without scanning the chrome.
 */
.debug-bubble {
  position: fixed;
  right: 1rem;
  bottom: calc(5rem + env(safe-area-inset-bottom));
  z-index: 70;
  display: grid;
  place-items: center;
  width: 3.25rem;
  height: 3.25rem;
  color: #f8fbff;
  cursor: pointer;
  background:
    radial-gradient(120% 120% at 28% 18%, rgb(255 255 255 / 34%), transparent 46%),
    linear-gradient(145deg, var(--synra-primary), color-mix(in oklab, var(--synra-primary) 58%, #06121f));
  border: 1px solid rgb(255 255 255 / 26%);
  border-radius: 999px;
  box-shadow:
    0 10px 26px -8px color-mix(in oklab, var(--synra-primary) 72%, transparent),
    0 2px 6px rgb(0 0 0 / 28%),
    inset 0 1px 0 rgb(255 255 255 / 28%);
  transition: transform 160ms ease, box-shadow 160ms ease;
}

.debug-bubble:hover {
  transform: translateY(-1px) scale(1.03);
  box-shadow:
    0 14px 32px -8px color-mix(in oklab, var(--synra-primary) 84%, transparent),
    0 2px 8px rgb(0 0 0 / 32%),
    inset 0 1px 0 rgb(255 255 255 / 34%);
}

.debug-bubble:focus-visible {
  outline: 2px solid #fff;
  outline-offset: 3px;
}

.debug-bubble-icon {
  width: 1.35rem;
  height: 1.35rem;
}

.debug-bubble-glow {
  position: absolute;
  inset: -0.35rem;
  pointer-events: none;
  border: 1px solid color-mix(in oklab, var(--synra-primary) 55%, transparent);
  border-radius: inherit;
  opacity: 0.55;
}

.debug-bubble-pulse .debug-bubble-glow {
  animation: debug-ring 900ms ease-out;
}

.debug-bubble-badge {
  position: absolute;
  top: -0.2rem;
  right: -0.15rem;
  min-width: 1.15rem;
  padding: 0 0.25rem;
  color: #06121f;
  font-size: 0.62rem;
  font-weight: 700;
  line-height: 1.15rem;
  text-align: center;
  background: #ffd479;
  border: 1px solid rgb(0 0 0 / 18%);
  border-radius: 999px;
}

@keyframes debug-ring {
  from { opacity: 0.8; transform: scale(0.85); }
  to { opacity: 0; transform: scale(1.5); }
}

.debug-layer {
  position: fixed;
  inset: 0;
  z-index: 80;
  pointer-events: none;
}

.debug-layer-open {
  pointer-events: auto;
}

.debug-scrim {
  position: absolute;
  inset: 0;
  background: rgb(3 7 12 / 42%);
  backdrop-filter: blur(2px);
  opacity: 0;
  transition: opacity 220ms ease;
}

.debug-layer-open .debug-scrim {
  opacity: 1;
}

.debug-sheet {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  display: flex;
  flex-direction: column;
  height: min(78vh, 46rem);
  overflow: hidden;
  color: #dbe4f0;
  background:
    radial-gradient(80rem 20rem at 50% -32%, color-mix(in oklab, var(--synra-primary) 16%, transparent), transparent),
    rgb(9 13 19 / 96%);
  border-top: 1px solid rgb(255 255 255 / 12%);
  border-top-left-radius: 1.15rem;
  border-top-right-radius: 1.15rem;
  box-shadow: 0 -18px 60px -24px rgb(0 0 0 / 70%), inset 0 1px 0 rgb(255 255 255 / 6%);
  backdrop-filter: blur(22px) saturate(135%);
  transform: translateY(102%);
  transition: transform 280ms cubic-bezier(0.22, 1, 0.36, 1);
}

.debug-layer-open .debug-sheet {
  transform: translateY(0);
}

.debug-grip {
  width: 2.6rem;
  height: 0.25rem;
  margin: 0.45rem auto 0.1rem;
  background: rgb(255 255 255 / 18%);
  border-radius: 999px;
}

.debug-head {
  display: flex;
  flex: none;
  gap: 0.6rem;
  align-items: center;
  padding: 0.28rem 0.85rem 0.32rem;
  border-bottom: 1px solid rgb(255 255 255 / 7%);
}

.debug-heading {
  min-width: 0;
}

.debug-title {
  display: flex;
  gap: 0.45rem;
  align-items: baseline;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 0.78rem;
  font-weight: 600;
  color: #f2f7ff;
}

.debug-scheme {
  font-size: 0.66rem;
  font-weight: 400;
  color: #6f829b;
}

.debug-counters {
  display: flex;
  gap: 0.4rem;
  align-items: center;
  margin-left: auto;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 0.66rem;
}

.debug-counter {
  padding: 0.12rem 0.4rem;
  color: #93a6bf;
  background: rgb(255 255 255 / 5%);
  border: 1px solid rgb(255 255 255 / 7%);
  border-radius: 999px;
}

.debug-counter-sent { color: #67e8f9; }
.debug-counter-received { color: #86efac; }

.debug-actions {
  display: flex;
  flex: none;
  gap: 0.15rem;
  align-items: center;
}

.debug-action {
  padding: 0.22rem 0.5rem;
  color: #a8b8cd;
  font: inherit;
  font-size: 0.68rem;
  cursor: pointer;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 0.45rem;
  transition: color 120ms ease, background 120ms ease, border-color 120ms ease;
}

.debug-action:hover {
  color: #eef5ff;
  background: rgb(255 255 255 / 7%);
  border-color: rgb(255 255 255 / 8%);
}

.debug-action-close {
  display: grid;
  place-items: center;
  width: 1.65rem;
  height: 1.65rem;
  padding: 0;
}

.debug-action-close svg {
  width: 0.9rem;
  height: 0.9rem;
}

.debug-filters {
  display: flex;
  flex: none;
  gap: 0.35rem;
  align-items: center;
  padding: 0.4rem 0.85rem;
  border-bottom: 1px solid rgb(255 255 255 / 5%);
}

.debug-filter {
  padding: 0.15rem 0.55rem;
  color: #8295ad;
  font: inherit;
  font-size: 0.66rem;
  cursor: pointer;
  background: transparent;
  border: 1px solid rgb(255 255 255 / 8%);
  border-radius: 999px;
}

.debug-filter:hover {
  color: #dbe4f0;
}

.debug-filter-on {
  color: #06121f;
  background: #cfe4ff;
  border-color: #cfe4ff;
}

.debug-filter-sent.debug-filter-on {
  background: #7ee8f5;
  border-color: #7ee8f5;
}

.debug-filter-received.debug-filter-on {
  background: #9df3b8;
  border-color: #9df3b8;
}

.debug-hint {
  margin-left: auto;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 0.62rem;
  color: #5e7189;
}

.debug-term-wrap {
  position: relative;
  flex: 1;
  min-height: 0;
  padding: 0.45rem 0.55rem calc(0.7rem + env(safe-area-inset-bottom));
}

.debug-term {
  width: 100%;
  height: 100%;
}

.debug-empty {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  pointer-events: none;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 0.72rem;
  color: #52647b;
}

@media (min-width: 48rem) {
  .debug-bubble {
    right: 1.5rem;
    bottom: 1.5rem;
    width: 3.4rem;
    height: 3.4rem;
  }

  .debug-sheet {
    right: 1.25rem;
    left: 1.25rem;
    height: min(72vh, 42rem);
    border: 1px solid rgb(255 255 255 / 10%);
    border-bottom: 0;
    border-radius: 1.15rem 1.15rem 0 0;
  }
}
</style>

<!-- xterm paints its scrollbar into the DOM at runtime, so its width cannot be scoped. -->
<style>
.debug-sheet .xterm-scrollable-element > .scrollbar.vertical {
  width: 10px !important;
}

.debug-sheet .xterm-scrollable-element > .scrollbar.vertical > .slider {
  left: 1px !important;
  width: 8px !important;
  border-radius: 9999px;
}
</style>