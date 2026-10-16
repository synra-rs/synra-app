<template>
  <div class="flex flex-col gap-2 panel p-4">
    <p class="row gap-2 text-base text-danger">
      <span class="i-carbon-warning-alt shrink-0 text-lg" aria-hidden="true" />
      <span>{{ headline }}</span>
    </p>

    <!--
      The kernel's suggested actions, rendered rather than dropped (R33).
      Each one is a thing the user can do next; the ones the interface cannot carry out yet are shown as a direction
      rather than as a button that would do nothing.
    -->
    <div v-if="actions.length > 0" class="row flex-wrap gap-2">
      <template v-for="(action, index) in actions" :key="index">
        <RouterLink v-if="to(action)" :to="to(action)!" class="border border-line-strong rounded-control px-3 py-1.5 text-sm text-primary-ink focusable @hover:bg-raised">
          {{ messages.actions[action.kind] }}
        </RouterLink>
        <button v-else-if="action.kind === 'retry'" type="button" class="border border-line-strong rounded-control px-3 py-1.5 text-sm text-primary-ink focusable @hover:bg-raised" @click="emit('retry')">
          {{ messages.actions.retry }}
        </button>
        <span v-else class="text-sm text-ink-dim">{{ messages.actions[action.kind] }}</span>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * The interface's error surface, in one place (R33, R34).
 *
 * # Why this is a component and not four paragraphs
 *
 * Every page can fail the same way — the kernel refuses a command — and a refusal carries `actions` the user is supposed
 * to be able to take. Four hand-written error paragraphs is four places to forget them; this is one place to get it
 * right, and the four pages then only decide *which* failures they show.
 *
 * # What it deliberately does not show
 *
 * `error.message` and `error.code`. The message is written for whoever implements the kernel (R34 keeps words like
 * "kernel", "snapshot" and "refused" out of the interface), and the code belongs on the diagnostics page. What is shown
 * is our own sentence for that code, plus the actions.
 */
import type { SuggestedAction } from '@synra/protocol'
import { computed } from 'vue'

import { messages } from '../messages'

const props = defineProps<{
  readonly code: string
  /** The kernel's message **key** (not a sentence); see `messages.ts` for why it is consulted first. */
  readonly message?: string | undefined
  readonly actions?: readonly SuggestedAction[] | undefined
}>()

const emit = defineEmits<{
  retry: []
}>()

const headline = computed(() =>
  messages.kernelErrors[props.message as keyof typeof messages.kernelErrors]
  ?? messages.errors[props.code as keyof typeof messages.errors]
  ?? messages.errors.unknown
)
const actions = computed<readonly SuggestedAction[]>(() => props.actions ?? [])

/** Where an action points, when the interface has a screen for it. */
function to(action: SuggestedAction): string | undefined {
  switch (action.kind) {
    case 'openSettings': return '/settings'
    case 'connectDevice': return '/devices'
    case 'installPlugin': return '/plugins'
    default: return undefined
  }
}
</script>
