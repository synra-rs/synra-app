<template>
  <AAlert type="error" show-icon :message="headline">
    <!--
      The kernel's suggested actions, rendered rather than dropped (R33) — and rendered with antd's own buttons,
      because a control is the component library's job (R51). The ones the interface cannot carry out yet are shown as
      a direction rather than as a button that would do nothing.
    -->
    <template v-if="actions.length > 0" #description>
      <div class="row flex-wrap gap-2 pt-1">
        <template v-for="(action, index) in actions" :key="index">
          <AButton v-if="to(action)" size="small" @click="router.push(to(action)!)">
            {{ messages.actions[action.kind] }}
          </AButton>
          <AButton v-else-if="action.kind === 'retry'" size="small" type="primary" @click="emit('retry')">
            {{ messages.actions.retry }}
          </AButton>
          <span v-else class="text-xs text-ink-dim">{{ messages.actions[action.kind] }}</span>
        </template>
      </div>
    </template>
  </AAlert>
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
 * # Why it is an `a-alert` and not a styled div
 *
 * Because it is a *control surface*: an error with buttons on it. R51 puts controls in the component library — and that
 * is what makes it look like the rest of the interface rather than like the one hand-rolled thing in it. Its colours are
 * not antd's defaults either: `type="error"` resolves to the theme's `danger` token through `synraAntdTheme()`.
 *
 * # What it deliberately does not show
 *
 * `error.code`. The host's `message` is a **key** (`error.bridgeFailed`), not a sentence, so the sentence is ours:
 * `messages.ts` renders it, and the code is left for the diagnostics page (R34 keeps words like "kernel", "snapshot"
 * and "refused" out of the interface).
 */
import type { SuggestedAction } from '@synra/protocol'
import { Alert as AAlert, Button as AButton } from 'ant-design-vue'
import { computed } from 'vue'
import { useRouter } from 'vue-router'

import { messages } from '../messages'
import { errorSentence } from '../messages/errors'

const props = defineProps<{
  readonly code: string
  readonly actions?: readonly SuggestedAction[] | undefined
}>()

const emit = defineEmits<{
  retry: []
}>()

const router = useRouter()

const headline = computed(() => errorSentence(props.code))
const actions = computed<readonly SuggestedAction[]>(() => props.actions ?? [])

/**
 * Where an action points, when the interface has a screen for it.
 *
 * ★ Both of these carry the object the kernel named, and both pages now read it: `PluginsPage` opens the row for
 * `?plugin=`, `DevicesPage` opens the pairing panel for `?device=`. Dropping the id landed the reader on the top of a
 * list and left them to find the plugin or device the error had already named.
 */
function to(action: SuggestedAction): string | undefined {
  switch (action.kind) {
    case 'openSettings': return '/settings'
    case 'connectDevice': return `/devices?device=${encodeURIComponent(action.deviceId)}`
    case 'installPlugin': return `/plugins?plugin=${encodeURIComponent(action.pluginId)}`
    default: return undefined
  }
}
</script>
