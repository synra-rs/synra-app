<template>
  <AAlert type="error" show-icon :message="headline">
    <template #description>
      <!--
        ★ The kernel's own explanation, when it wrote one.

        A refusal carries a `code`, and that code's key names a *class* of failure — `invalid_argument` renders as
        "Something in the request was not valid." — while the same code covers every different reason the kernel can
        have for it. The reason itself is in `params.detail`/`params.reason`, and until this block existed the
        interface dropped it: pairing wrote "the selected device has no discovery route" and the reader got the generic
        sentence, with no way to tell one cause from another and nothing to act on.

        Above the actions, because it explains them — "check the invitation" reads differently once you know it was
        the device id that did not parse.
      -->
      <p v-if="detail" data-testid="error-detail" class="m-0 break-words text-sm">
        {{ detail }}
      </p>
      <!--
        The kernel's suggested actions, rendered rather than dropped (R33) — and rendered with antd's own buttons,
        because a control is the component library's job (R51). The ones the interface cannot carry out yet are shown
        as a direction rather than as a button that would do nothing.
      -->
      <!--
        ★ `` is the same floor every other surface states
          ([09 §3]): 44px where a finger is the pointer, antd's own 32px from `md` up. `size="small"` is 24px, so
          these were the only controls in the interface a thumb could miss — on the one notice that appears when
          something has already gone wrong.
      -->
      <div
        v-if="actions.length > 0"
        data-testid="error-actions"
        class="row flex-wrap gap-2 pt-1"
        :class="{ 'pt-2': detail }"
      >
        <template v-for="(action, index) in actions" :key="index">
          <AButton v-if="to(action)" size="small" @click="router.push(to(action)!)">
            {{ messages.actions[action.kind] }}
          </AButton>
          <AButton v-else-if="action.kind === 'retry'" size="small" type="primary" @click="emit('retry')">
            {{ messages.actions.retry }}
          </AButton>
          <span v-else class="text-sm text-ink-dim">{{ messages.actions[action.kind] }}</span>
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
import { detailOf } from '../messages/failure-detail'

const props = defineProps<{
  readonly code: string
  readonly actions?: readonly SuggestedAction[] | undefined
  /**
   * The kernel's own parameters for the refusal.
   *
   * Passed through rather than pre-rendered so every call site does not have to know which key the kernel used: see
   * `detailOf`. A caller that already holds a `Failure` (`pages/plugins/actions.ts`) can pass `:detail` instead.
   */
  readonly params?: unknown
  /** A `Failure`'s already-extracted explanation, when the caller has one. */
  readonly detail?: string | undefined
}>()

const emit = defineEmits<{
  retry: []
}>()

const router = useRouter()

const headline = computed(() => errorSentence(props.code))
const actions = computed<readonly SuggestedAction[]>(() => props.actions ?? [])
const detail = computed(() => props.detail ?? detailOf(props.params))

/**
 * Where an action points, when the interface has a screen for it.
 *
 * ★ All three carry the object the kernel named, and all three pages read it: `PluginsPage` opens the row for
 * `?plugin=`, `DevicesPage` opens the pairing panel for `?device=`, `SettingsPage` opens the column for `?category=`.
 * Dropping the id landed the reader on the top of a list, or on the first settings column, and left them to find what
 * the error had already named.
 *
 * `section` is the kernel's own key and is passed through unvalidated: an unknown one falls back to the first column,
 * which is what dropping it did every time.
 */
function to(action: SuggestedAction): string | undefined {
  switch (action.kind) {
    case 'openSettings': return `/settings?category=${encodeURIComponent(action.section)}`
    case 'connectDevice': return `/devices?device=${encodeURIComponent(action.deviceId)}`
    case 'installPlugin': return `/plugins?plugin=${encodeURIComponent(action.pluginId)}`
    default: return undefined
  }
}
</script>
