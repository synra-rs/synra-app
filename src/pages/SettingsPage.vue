<template>
  <section class="stack gap-6">
    <h1 class="text-xl font-semibold">
      {{ messages.settings.title }}
    </h1>

    <ErrorNotice v-if="error" :code="error.code" :message="error.message" :actions="error.actions" @retry="load" />

    <section class="stack gap-3">
      <h2 class="text-base font-medium">
        {{ messages.settings.device }}
      </h2>

      <!--
        The device's own facts, as a labelled table rather than a two-column `<dl>` that only looks like one. The id
        and the version are machine-issued strings, which is what monospace is for (R21).
      -->
      <ADescriptions v-if="identity" :column="1" bordered>
        <ADescriptions.Item :label="messages.settings.field.name">
          <div class="stack gap-0.5">
            <span>{{ identity.displayName }}</span>
            <!-- The hint is a line of its own, not a parenthetical after the name (R21 retires `WORD — fragment`). -->
            <span v-if="identity.nameIsDefault" class="text-xs text-ink-dim">
              {{ messages.settings.field.nameIsGenerated }}
            </span>
          </div>
        </ADescriptions.Item>

        <ADescriptions.Item :label="messages.settings.field.platform">
          {{ identity.platform }}
        </ADescriptions.Item>

        <ADescriptions.Item :label="messages.settings.field.kernel">
          <span class="font-mono">v{{ identity.kernelVersion }}</span>
        </ADescriptions.Item>

        <ADescriptions.Item :label="messages.settings.field.deviceId">
          <span class="font-mono">{{ identity.deviceId }}</span>
        </ADescriptions.Item>
      </ADescriptions>
    </section>

    <section class="stack gap-3">
      <h2 class="text-base font-medium">
        {{ messages.settings.preferences }}
      </h2>

      <!--
        The reason a control does nothing appears twice on purpose. It is on the controls, because that is where the
        question is asked; and it is here on the page, because a disabled control cannot take focus, so for a keyboard
        user the tooltip is unreachable — and "it is disabled" with no reason is the thing this page's doc comment
        refuses to do.
      -->
      <p class="m-0 text-sm text-ink-dim">
        {{ messages.settings.notWiredYet }}
      </p>

      <ATooltip :title="messages.settings.notWiredYet">
        <div class="stack gap-3">
          <div v-for="key in KEYS" :key="key" class="row-between gap-4">
            <span class="min-w-0 row gap-2">
              <span class="text-sm">{{ messages.settings.preference[key] }}</span>
              <!--
                The value as the kernel answered it, printed rather than prettified. `null` is a fact — the kernel has
                no value for that key — and the controls below take their state from the same read, so nothing here is
                a default this page invented.
              -->
              <span class="truncate text-xs text-ink-dim font-mono">{{ JSON.stringify(config[key] ?? null) }}</span>
            </span>

            <ASwitch v-if="key === 'autoSyncPlugins'" :checked="config[key] === true" disabled />

            <!--
              A select with no options: the set of themes is the kernel's to name, and a list we wrote here would be
              this page inventing values it does not own. Disabled, it shows the value it was given and nothing else.
            -->
            <ASelect v-else :value="preferenceValue(key)" disabled class="w-fit" />
          </div>
        </div>
      </ATooltip>
    </section>
  </section>
</template>

<script setup lang="ts">
/**
 * This device's own settings.
 *
 * # Why this page reads the kernel rather than `localStorage`
 *
 * A preference stored in the browser is a preference that does not follow the user to the other devices, does not appear
 * in a diagnostic bundle, and is not the thing `config_get` returns. The design document is explicit that configuration
 * is the kernel's, so this page round-trips a real command even though the value it changes is not written back yet.
 *
 * # Why nothing is written back in M0
 *
 * `config_set` exists and is not called from here. A toggle that appears to work and does not is worse than one that is
 * visibly absent: the user changes a setting, sees the shell's own state move, restarts the app, and is now debugging a
 * bug in the settings page. So the control is rendered disabled, with the reason.
 */
import type { IdentitySnapshot, SuggestedAction } from '@synra/protocol'
import { Descriptions as ADescriptions, Select as ASelect, Switch as ASwitch, Tooltip as ATooltip } from 'ant-design-vue'
import { onMounted, ref } from 'vue'

import ErrorNotice from '../components/ErrorNotice.vue'
import { kernel } from '../kernel'
import { messages } from '../messages'

/** The keys this page knows about, in the order they are shown. */
const KEYS = ['theme', 'autoSyncPlugins'] as const

type PreferenceKey = typeof KEYS[number]

const config = ref<Record<string, unknown>>({})
const identity = ref<IdentitySnapshot | undefined>(undefined)

/**
 * The kernel's answer for a key, as a select can show it.
 *
 * A select holds scalars, and the kernel's bag is `unknown`, so anything that is not one is passed to the control as
 * "no value" rather than stringified into a value this page made up.
 */
function preferenceValue(key: PreferenceKey): string | number | undefined {
  const value = config.value[key]
  return typeof value === 'string' || typeof value === 'number' ? value : undefined
}

/** The refusal itself, not its sentence: the surface needs the `code`, the message **key** and the `actions` (R33). */
const error = ref<{ code: string, message: string, actions?: readonly SuggestedAction[] | undefined } | undefined>(undefined)

onMounted(load)

/** Reads what this device is and what it is set to. Both answers arrive together, because one screen shows both. */
async function load(): Promise<void> {
  error.value = undefined

  const [configAnswer, identityAnswer] = await Promise.all([
    kernel.call<Record<string, unknown>>('config_get'),
    kernel.call<IdentitySnapshot>('identity_get')
  ])

  if (configAnswer.ok)
    config.value = configAnswer.value
  if (identityAnswer.ok)
    identity.value = identityAnswer.value

  // Both refusals are reported rather than swallowed: a panel that renders "no value" when the kernel could not be
  // asked is worse than an empty one, because the empty value is an answer.
  const refusal = [configAnswer, identityAnswer].find(answer => !answer.ok)
  if (refusal && !refusal.ok)
    error.value = { code: refusal.error.code, message: refusal.error.message, actions: refusal.error.actions }
}
</script>
