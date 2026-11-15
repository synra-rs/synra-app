<template>
  <Teleport to="body">
    <button
      v-show="!open && enabled"
      ref="bubble"
      type="button"
      class="debug-bubble border-0 bg-transparent"
      :aria-label="words.open"
      :title="words.open"
      @click="toggle"
    >
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
    <div
      class="debug-layer"
      :class="{ 'debug-layer-open': visible }"
      :aria-hidden="!visible"
      :inert="!visible"
    >
      <div class="debug-scrim" @click="hide" />
      <section class="debug-sheet" role="dialog" aria-modal="true" :aria-label="words.title">
        <div class="debug-grip" aria-hidden="true" />
        <header class="debug-head">
          <div class="debug-live" role="status" aria-live="polite">
            <span class="debug-live-dot" aria-hidden="true">●</span>
            <span>LIVE</span>
          </div>
          <button
            ref="closeButton"
            type="button"
            class="debug-action debug-action-close border-0 bg-transparent"
            :aria-label="words.close"
            @click="hide"
          >
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path d="m4 4 8 8M12 4l-8 8" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
            </svg>
          </button>
        </header>
        <div class="debug-filters" role="toolbar" :aria-label="words.tools">
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
          <details class="debug-tools">
            <summary class="debug-tools-summary" :aria-label="words.tools">⋯</summary>
            <div class="debug-tools-popover">
              <button type="button" class="debug-action border-0 bg-transparent" @click="paused = !paused">
                {{ paused ? words.resume : words.pause }}
              </button>
              <button type="button" class="debug-action border-0 bg-transparent" @click="clear">
                {{ words.clear }}
              </button>
              <button type="button" class="debug-action border-0 bg-transparent" @click="copy">
                {{ copyLabel }}
              </button>
            </div>
          </details>
        </div>
        <div class="debug-term-wrap">
          <div ref="host" class="debug-term" />
          <div v-if="lineCount === 0" class="debug-empty">
            {{ words.empty }}
          </div>
        </div>
      </section>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { useTemplateRef } from 'vue'
import { useWireConsole } from '../debug/use-wire-console'
import '../debug/bubble.css'
import '../debug/console.css'
import '../debug/terminal.css'

const host = useTemplateRef<HTMLDivElement>('host')
const bubble = useTemplateRef<HTMLButtonElement>('bubble')
const closeButton = useTemplateRef<HTMLButtonElement>('closeButton')
const { open, visible, paused, filter, unread, lineCount, enabled, words, hide, toggle, clear, copy, copyLabel } = useWireConsole(host, bubble, closeButton)
</script>
