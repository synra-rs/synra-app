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
        The reason a control does nothing appears where the control would be, not as a page-wide paragraph: the switch
        writes, and the theme row says why there is no second choice.
      -->
      <div class="stack gap-3">
        <div v-for="key in KEYS" :key="key" class="row-between gap-4">
          <span class="min-w-0 row gap-2">
            <span class="text-sm">{{ messages.settings.preference[key] }}</span>
            <!--
                The value as the kernel answered it, printed rather than prettified. `null` is a fact — the kernel has
                no value for that key — and the controls below take their state from the same read, so nothing here is
                a default this page invented.
              -->
            <span class="truncate text-xs text-ink-dim font-mono">{{ JSON.stringify(settings.value(configKey(key)) ?? null) }}</span>
          </span>

          <ASwitch
            v-if="key === 'autoSyncPlugins'"
            :checked="settings.value(configKey(key)) === true"
            :loading="saving === key"
            @change="(checked) => save(key, checked === true)"
          />

          <!--
              ★ One theme, and the sentence says so. A select with no options looked like a control the page had failed
              to fill; `ui.theme` is a record the kernel keeps and plugins read, not a preference this device can change
              (28 §6 R22). The value itself is printed above.
            -->
          <span v-else class="text-sm text-ink-dim">{{ messages.settings.themeFixed }}</span>
        </div>
      </div>
    </section>

    <section class="stack gap-3">
      <h2 class="m-0 text-base font-medium">
        {{ messages.settings.transfers.title }}
      </h2>
      <div class="row-between gap-4">
        <div class="stack">
          <span class="text-sm">{{ messages.settings.transfers.cancelAsk }}</span>
          <span class="text-xs text-ink-dim">{{ messages.settings.transfers.cancelAskDetail }}</span>
        </div>
        <ASwitch
          :checked="askCancel"
          :loading="savingCancel"
          @change="(checked) => saveCancel(checked === true)"
        />
      </div>
    </section>

    <section class="stack gap-3">
      <h2 class="text-base font-medium">
        {{ messages.settings.trusted.title }}
      </h2>
      <p class="m-0 text-sm text-ink-dim">
        {{ messages.settings.trusted.detail }}
      </p>

      <!--
        The list is the configuration key the transfer prompt writes (`transfers.acceptFrom`), not a second store. Removing
        a row writes the key back (or unsets it when the last row goes), so there is one answer to "may this device send
        without asking".
      -->
      <AEmpty v-if="trusted.length === 0" :description="messages.settings.trusted.empty" />
      <AList v-else size="small">
        <AList.Item v-for="device in trusted" :key="device.id">
          <div class="row-between gap-3">
            <span class="min-w-0 row gap-2">
              <span class="truncate text-sm">{{ device.name }}</span>
              <span v-if="device.name !== device.id" class="truncate text-xs text-ink-dim font-mono">{{ device.id }}</span>
            </span>
            <AButton
              size="small"
              :loading="forgetting === device.id"
              @click="forgetTrusted(device.id)"
            >
              {{ messages.settings.trusted.remove }}
            </AButton>
          </div>
        </AList.Item>
      </AList>
    </section>

    <!--
      The registry is a URL, not a preference among named values, so it gets its own input. Empty means "no registry":
      the install path stays available from a directory, and the registry branch refuses by name instead of dialling a
      default somebody else chose.
    -->
    <section class="stack gap-3">
      <h2 class="m-0 text-base font-medium">
        {{ messages.settings.registry.title }}
      </h2>
      <p class="m-0 text-sm text-ink-dim">
        {{ messages.settings.registry.detail }}
      </p>
      <div v-for="(_, index) in registryUrls" :key="index" class="row gap-2">
        <AInput
          :value="registryUrls[index] ?? ''"
          :placeholder="messages.settings.registry.placeholder"
          allow-clear
          @update:value="(value: string) => updateRegistry(index, value)"
          @press-enter="saveRegistry"
        />
        <AButton
          v-if="registryUrls.length > 1"
          @click="removeRegistry(index)"
        >
          {{ messages.settings.registry.remove }}
        </AButton>
      </div>
      <div class="row gap-2">
        <AButton @click="addRegistry">
          {{ messages.settings.registry.add }}
        </AButton>
        <AButton :loading="savingRegistry" @click="saveRegistry">
          {{ messages.settings.registry.save }}
        </AButton>
      </div>
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
 * # What is written back, and what is not
 *
 * The automatic-update switch and the registry URL both round-trip `config_set` and let the kernel's `ConfigUpdated`
 * event feed the store. The theme select does not: `@synra/theme` ships one palette, so a select that offered others would
 * be the fake control this page's doc comment refuses to draw.
 */
import type { ConfigSnapshot, IdentitySnapshot, SuggestedAction } from '@synra/protocol'
import {
  Button as AButton,
  Descriptions as ADescriptions,
  Empty as AEmpty,
  Input as AInput,
  List as AList,
  Switch as ASwitch
} from 'ant-design-vue'
import { computed, onMounted, ref, watch } from 'vue'

import ErrorNotice from '../components/ErrorNotice.vue'
import { kernel } from '../kernel'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'
import { useSettingsStore } from '../stores/settings'

/** The keys this page knows about, in the order they are shown. */const KEYS = ['theme', 'autoSyncPlugins'] as const

type PreferenceKey = typeof KEYS[number]

// ★ The store, not a copy of the answer. Two things this fixes: another window's change arrives as `configUpdated` and
// lands here, and — the defect this line replaced — the command answers a **`ConfigSnapshot`** (`{ values, overridden }`),
// not a bare map, so a page that stored the answer as the map read `config['theme']` as `undefined` forever.
const settings = useSettingsStore()
const identity = ref<IdentitySnapshot | undefined>(undefined)

/**
 * The kernel's key for one of this page's preferences.
 *
 * ★ The page's names (`theme`) are **labels**; the kernel addresses configuration by dotted path (`ui.theme`). Reading
 * `key` directly returned `undefined` for every preference — the second half of the bug that made both controls show
 * "unset", and the half a corrected DTO shape alone would not have fixed.
 */
function configKey(key: PreferenceKey): string {
  return `ui.${key}`
}

/** The refusal itself, not its sentence: the surface needs the `code`, the message **key** and the `actions` (R33). */
const error = ref<{ code: string, message: string, actions?: readonly SuggestedAction[] | undefined } | undefined>(undefined)

/** The key being written, so its control can say so and cannot be pressed twice. */
const saving = ref<PreferenceKey | undefined>(undefined)

/**
 * The devices this one accepts files from without asking.
 *
 * The configuration holds identifiers; the devices store holds the names it knows. A device that is no longer in the
 * list is shown by its identifier rather than hidden — the preference is still real, and a row that vanished would be a
 * setting the user cannot see or remove.
 */
const devices = useDevicesStore()

const trusted = computed<readonly { id: string, name: string }[]>(() => {
  const value = settings.value('transfers.acceptFrom')
  if (!Array.isArray(value))
    return []
  return value
    .filter((id): id is string => typeof id === 'string')
    .map((id) => {
      const card = devices.cards.find(entry => entry.deviceId === id)
      return { id, name: card?.displayName ?? id }
    })
})

/** Whether an in-flight transfer asks before it is abandoned; absent means yes, because the safe default is the question. */
const askCancel = ref(true)

/** Whether that preference is being written. */
const savingCancel = ref(false)

watch(
  () => settings.value('transfers.confirmCancel'),
  (value) => {
    askCancel.value = value !== false
  },
  { immediate: true }
)

/** Writes the cancel-confirmation preference. */
async function saveCancel(value: boolean): Promise<void> {
  savingCancel.value = true
  error.value = undefined
  const answer = await kernel.call<null>('config_set', {
    patch: { set: { 'transfers.confirmCancel': value } }
  })
  savingCancel.value = false
  if (!answer.ok)
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

/** The device being un-trusted, so its button can say so. */
const forgetting = ref<string | undefined>(undefined)

/** The registries being edited, in the order they will be tried; empty means the feature is off. */
const registryUrls = ref<string[]>([''])

/** Whether the registry list is being written, so the button can say so and cannot be pressed twice. */
const savingRegistry = ref(false)

// The store is the truth — another window's change lands here through `ConfigUpdated` — and the inputs are a buffer over
// it rather than a second copy that would disagree the first time the kernel normalised the list.
watch(
  () => settings.value('plugins.registry'),
  (value) => {
    if (typeof value === 'string') {
      registryUrls.value = [value]
    }
    else if (Array.isArray(value)) {
      registryUrls.value = value.filter((entry): entry is string => typeof entry === 'string')
    }
    else {
      registryUrls.value = ['']
    }
    if (registryUrls.value.length === 0)
      registryUrls.value = ['']
  },
  { immediate: true }
)

/** Adds an empty row to fill in. */
function addRegistry(): void {
  registryUrls.value.push('')
}

/** Writes one row's value. A handler rather than `v-model` on `registryUrls[index]`: `noUncheckedIndexedAccess` makes
 * that expression `string | undefined`, and the input's `value` is not optional. */
function updateRegistry(index: number, value: string): void {
  registryUrls.value[index] = value
}

/** Removes one row; the last removal is a cleared field, which the save turns into an unset. */
function removeRegistry(index: number): void {
  registryUrls.value.splice(index, 1)
}

/** Writes the list: one URL as a string (the shape older configs hold), several as an array, none as an unset. */
async function saveRegistry(): Promise<void> {
  savingRegistry.value = true
  error.value = undefined
  const urls = registryUrls.value.map(url => url.trim()).filter(url => url !== '')
  const patch = urls.length === 0
    ? { unset: ['plugins.registry'] }
    : { set: { 'plugins.registry': urls.length === 1 ? urls[0] : urls } }
  const answer = await kernel.call<null>('config_set', { patch })
  savingRegistry.value = false
  if (!answer.ok)
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

/**
 * Takes back "always accept" for one device.
 *
 * The last row unsets the key instead of storing an empty list: a settings screen should not have to interpret "none" as
 * a preference, and the kernel's own default is "ask" — which is the absence of a row.
 */
async function forgetTrusted(deviceId: string): Promise<void> {
  forgetting.value = deviceId
  error.value = undefined
  const remaining = trusted.value.filter(device => device.id !== deviceId).map(device => device.id)
  const patch = remaining.length > 0
    ? { set: { 'transfers.acceptFrom': remaining } }
    : { unset: ['transfers.acceptFrom'] }
  const answer = await kernel.call<null>('config_set', { patch })
  forgetting.value = undefined
  if (!answer.ok)
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

/**
 * Writes one preference.
 *
 * # Why it does not touch the store
 *
 * Because the kernel announces the change (`ConfigUpdated`) and `src/events.ts` puts it in the store. Writing locally too
 * would be a second description of the same fact — and the two would disagree the first time the kernel normalised a value
 * or refused the write.
 */
async function save(key: PreferenceKey, value: boolean): Promise<void> {
  saving.value = key
  error.value = undefined
  const patch = { set: { [configKey(key)]: value } }
  const answer = await kernel.call<null>('config_set', { patch })
  saving.value = undefined
  if (!answer.ok)
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

onMounted(load)

/** Reads what this device is and what it is set to. Both answers arrive together, because one screen shows both. */
async function load(): Promise<void> {
  error.value = undefined

  const [configAnswer, identityAnswer] = await Promise.all([
    kernel.call<ConfigSnapshot>('config_get'),
    kernel.call<IdentitySnapshot>('identity_get')
  ])

  if (configAnswer.ok)
    settings.applySnapshot(configAnswer.value)
  if (identityAnswer.ok)
    identity.value = identityAnswer.value

  // Both refusals are reported rather than swallowed: a panel that renders "no value" when the kernel could not be
  // asked is worse than an empty one, because the empty value is an answer.
  const refusal = [configAnswer, identityAnswer].find(answer => !answer.ok)
  if (refusal && !refusal.ok)
    error.value = { code: refusal.error.code, message: refusal.error.message, actions: refusal.error.actions }
}
</script>
