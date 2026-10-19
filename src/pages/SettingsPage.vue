<template>
  <section class="stack gap-6">
    <h1 class="text-xl font-semibold">
      {{ messages.settings.title }}
    </h1>

    <ErrorNotice v-if="error" :code="error.code" :message="error.message" :actions="error.actions" @retry="reload" />

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

    <!--
      ★ `diag_metrics` had no caller before this panel, and 18 §4.2 names this panel as one of the three reasons the
      recorder exists. Each row is the key **as the kernel renders it**, so it can be matched against a log line or a
      diagnostic export rather than being a second naming scheme.
    -->
    <section class="stack gap-3">
      <div class="row-between gap-4">
        <h2 class="m-0 text-base font-medium">
          {{ messages.settings.metrics.title }}
        </h2>
        <AButton size="small" :loading="refreshing" @click="refreshMetrics">
          {{ messages.settings.metrics.refresh }}
        </AButton>
      </div>
      <p class="m-0 text-sm text-ink-dim">
        {{ messages.settings.metrics.detail }}
      </p>
      <!--
        Read by "is there a snapshot" rather than "is it non-empty": the empty sentence is an answer, and drawing it before
        the first read would claim the recorder has counted nothing when nobody has asked yet.
      -->
      <p v-if="metrics && metrics.overflowedTotal > 0" class="m-0 text-xs text-ink-dim">
        {{ messages.settings.metrics.overflow(metrics.overflowedTotal, metrics.maxSeriesPerMetric) }}
      </p>
      <AEmpty v-if="metrics && metrics.values.length === 0" :description="messages.settings.metrics.empty" />
      <AList v-else-if="metrics" size="small">
        <AList.Item v-for="series in metrics.values" :key="seriesKey(series.key)">
          <div class="row-between gap-3">
            <span class="min-w-0 truncate text-xs font-mono">{{ seriesKey(series.key) }}</span>
            <span class="text-sm font-mono">{{ seriesValue(series.value) }}</span>
          </div>
        </AList.Item>
      </AList>
    </section>

    <!--
      ★ Last, and that is deliberate: every other section here edits a preference the screen owns; this one edits a number
      the kernel applies when it next starts, so it reads as housekeeping rather than as a control a user watches.
    -->
    <section class="stack gap-3">
      <h2 class="m-0 text-base font-medium">
        {{ messages.settings.audit.title }}
      </h2>
      <p class="m-0 text-sm text-ink-dim">
        {{ messages.settings.audit.detail }}
      </p>
      <div class="row gap-2">
        <AInputNumber
          :value="retentionDays"
          :min="1"
          :max="3650"
          :precision="0"
          @update:value="(value) => (retentionDays = typeof value === 'number' ? value : 1)"
        />
        <AButton :loading="savingRetention" @click="saveRetention">
          {{ messages.settings.audit.save }}
        </AButton>
      </div>
    </section>

    <!--
      ★ The bundle is the one thing on this page whose result the user has to *read back*: a file path alone says nothing
      about whether the audit trail went in. The two optional sections are checkboxes rather than defaults because both
      name things the user might not have meant to share.
    -->
    <section class="stack gap-3">
      <h2 class="m-0 text-base font-medium">
        {{ messages.settings.diagnostics.title }}
      </h2>
      <p class="m-0 text-sm text-ink-dim">
        {{ messages.settings.diagnostics.detail }}
      </p>
      <div class="stack gap-1">
        <ACheckbox
          :checked="exportAudit"
          @update:checked="(checked) => (exportAudit = checked)"
        >
          {{ messages.settings.diagnostics.audit }}
        </ACheckbox>
        <span class="text-xs text-ink-dim">{{ messages.settings.diagnostics.auditDetail }}</span>
      </div>
      <div class="stack gap-1">
        <ACheckbox
          :checked="exportDebug"
          @update:checked="(checked) => (exportDebug = checked)"
        >
          {{ messages.settings.diagnostics.debug }}
        </ACheckbox>
        <span class="text-xs text-ink-dim">{{ messages.settings.diagnostics.debugDetail }}</span>
      </div>
      <div v-if="exportDebug" class="row gap-2">
        <span class="text-sm">{{ messages.settings.diagnostics.lines }}</span>
        <AInputNumber
          :value="exportLines"
          :min="1"
          :max="10000"
          :precision="0"
          @update:value="(value) => (exportLines = typeof value === 'number' ? value : 1)"
        />
      </div>
      <div class="row gap-2">
        <AButton :loading="exporting" @click="exportDiagnostics">
          {{ messages.settings.diagnostics.export }}
        </AButton>
      </div>
      <div v-if="exported" class="stack gap-1">
        <span class="text-xs text-ink-dim font-mono">{{ exported.path }}</span>
        <span class="text-xs">{{ messages.settings.diagnostics.included(exported.included.join(', ')) }}</span>
        <span v-if="exported.excluded.length > 0" class="text-xs text-ink-dim">
          {{ messages.settings.diagnostics.excluded(exported.excluded.join('; ')) }}
        </span>
      </div>

      <!--
        ★ The crash prompt's "ignore". Safe mode is decided from the reports on disk, so leaving them behind means the
        next start asks again — this is the action that makes a fixed installation start normally.
      -->
      <div class="stack gap-2">
        <p class="m-0 text-xs text-ink-dim">
          {{ messages.settings.diagnostics.dismissDetail }}
        </p>
        <div class="row items-center gap-2">
          <AButton danger :loading="dismissing" @click="dismissCrashes">
            {{ messages.settings.diagnostics.dismissCrashes }}
          </AButton>
          <span v-if="crashesDismissed" class="text-xs text-ink-dim">
            {{ messages.settings.diagnostics.dismissed }}
          </span>
        </div>
      </div>

      <!--
        ★ Run on request, not on mount: the probes bind ports, write a file and read the clock, so running them on every
        visit would make navigating to settings do work the user did not ask for. The section explains what it checks
        because a user deciding whether to press it needs to know it is not a network test.
      -->
      <div class="stack gap-2">
        <p class="m-0 text-xs text-ink-dim">
          {{ messages.settings.diagnostics.probesDetail }}
        </p>
        <div class="row items-center gap-2">
          <AButton :loading="probing" @click="runProbes">
            {{ messages.settings.diagnostics.probes }}
          </AButton>
          <span v-if="probeReport" class="text-xs text-ink-dim">{{ probesSummary }}</span>
        </div>
        <ACheckbox
          v-if="probeReport"
          :checked="onlyProblems"
          @update:checked="(checked) => (onlyProblems = checked)"
        >
          {{ messages.settings.diagnostics.onlyProblems }}
        </ACheckbox>
        <ul v-if="probeReport" class="m-0 flex flex-col list-none gap-2 p-0">
          <li v-for="result in visibleProbes" :key="result.name" class="stack gap-1">
            <div class="row items-center gap-2">
              <ATag :color="probeColor(result.status)">
                {{ messages.settings.diagnostics.probeStatus(result.status) }}
              </ATag>
              <span class="text-xs font-mono">{{ result.name }}</span>
            </div>
            <span class="text-xs text-ink-dim">{{ result.detail }}</span>
            <span v-if="result.suggestion" class="text-xs">{{ result.suggestion }}</span>
          </li>
        </ul>
      </div>

      <!--
        ★ The log is read on demand, like the plugin audit: a page that read it on mount would hold a copy that goes stale
        while the user is looking at it, and a log can be the largest thing this page ever asks for. Opening the section
        is the request; closing it leaves what was read on screen, because the lines are still what the shell wrote.
      -->
      <details class="stack gap-2" @toggle="(event) => openLog(event)">
        <summary class="cursor-pointer text-xs text-ink-dim">
          {{ messages.settings.diagnostics.logs }}
        </summary>
        <pre v-if="logLines.length > 0" class="m-0 max-h-64 overflow-auto whitespace-pre-wrap break-all text-xs font-mono">{{ logLines.join('\n') }}</pre>
        <p v-else-if="logAsked" class="m-0 text-xs text-ink-dim">
          {{ messages.settings.diagnostics.logsEmpty }}
        </p>
      </details>
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
import type { ConfigSnapshot, DiagExportResult, IdentitySnapshot, MetricsSnapshot, MetricValue, ProbeReportDto, SeriesKey, SuggestedAction } from '@synra/protocol'
import {
  Button as AButton,
  Checkbox as ACheckbox,
  Descriptions as ADescriptions,
  Empty as AEmpty,
  Input as AInput,
  InputNumber as AInputNumber,
  List as AList,
  Switch as ASwitch,
  Tag as ATag
} from 'ant-design-vue'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'

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

/**
 * How many days recorded plugin calls are kept.
 *
 * The kernel's key is `audit.retentionDays`; seven is the design's default, and the kernel clamps whatever is stored to a
 * sane window. This is a buffer over the store for the same reason the registry list is: `ConfigUpdated` overwrites it.
 */
const retentionDays = ref(7)

/** Whether that setting is being written. */
const savingRetention = ref(false)

watch(
  () => settings.value('audit.retentionDays'),
  (value) => {
    retentionDays.value = typeof value === 'number' && Number.isFinite(value) ? value : 7
  },
  { immediate: true }
)

/** Writes the retention window, clamped here so the number in the box is the number that was sent. */
async function saveRetention(): Promise<void> {
  savingRetention.value = true
  error.value = undefined
  const days = Math.min(3_650, Math.max(1, Math.trunc(retentionDays.value)))
  retentionDays.value = days
  const answer = await kernel.call<null>('config_set', {
    patch: { set: { 'audit.retentionDays': days } }
  })
  savingRetention.value = false
  if (!answer.ok)
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

/** Whether the bundle carries the audit trail; it names the plugins the user runs, so it is off until asked for. */
const exportAudit = ref(false)

/** Whether it carries debug lines; the section most likely to hold something the user did not mean to share. */
const exportDebug = ref(false)

/** How many log lines to include when debug lines are on. */
const exportLines = ref(200)

/** Whether a bundle is being written. */
const exporting = ref(false)

/** What the last bundle holds, so a user can check it did not include more than they agreed to. */
const exported = ref<DiagExportResult | undefined>(undefined)

/** Writes a diagnostic bundle, and shows the path and the section list it wrote. */
async function exportDiagnostics(): Promise<void> {
  exporting.value = true
  error.value = undefined
  const options = {
    includeAudit: exportAudit.value,
    includeDebugLogs: exportDebug.value,
    ...(exportDebug.value ? { logLines: exportLines.value } : {})
  }
  const answer = await kernel.call<DiagExportResult>('diag_export', { options })
  exporting.value = false
  if (!answer.ok) {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  exported.value = answer.value
}

/** Whether the crash reports were dismissed in this visit. */
const crashesDismissed = ref(false)
/** The dismiss is in flight; a second click would be a second delete. */
const dismissing = ref(false)

/** Deletes the crash reports that make the next start enter safe mode. */
async function dismissCrashes(): Promise<void> {
  dismissing.value = true
  error.value = undefined
  const answer = await kernel.call<null>('diag_crash_dismiss')
  dismissing.value = false
  if (!answer.ok) {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  crashesDismissed.value = true
}

/** The probes' last report, or undefined before the user has asked for one. */
const probeReport = ref<ProbeReportDto | undefined>(undefined)

/** Whether the probes are running. */
const probing = ref(false)

/** Whether the list hides the passes: a report is useful as one line until something fails. */
const onlyProblems = ref(false)

/** Counts by status, read from the report rather than stored beside it — one source for both. */
const probesSummary = computed(() => {
  const results = probeReport.value?.results ?? []
  const count = (status: string): number => results.filter(result => result.status === status).length
  return messages.settings.diagnostics.probesSummary(count('ok'), count('warn'), count('fail'))
})

/** The rows the list draws: the whole report, or only the non-passing ones. */
const visibleProbes = computed(() => (probeReport.value?.results ?? [])
  .filter(result => !onlyProblems.value || result.status !== 'ok'))

/** A tag colour per status: the words are the same, the colour is what makes a failing report findable. */
function probeColor(status: string): string {
  return status === 'ok' ? 'green' : status === 'warn' ? 'orange' : 'red'
}

/** Runs the probes on request: they bind ports and touch the disk, so they are not part of loading the page. */
async function runProbes(): Promise<void> {
  probing.value = true
  error.value = undefined
  const answer = await kernel.call<ProbeReportDto>('diag_probes')
  probing.value = false
  if (!answer.ok) {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  probeReport.value = answer.value
}

/** The shell's recent log, once the section is opened. */
const logLines = ref<readonly string[]>([])

/** Whether the log has been asked for, so the empty sentence is not drawn before the first read. */
const logAsked = ref(false)

/** Reads the tail on every open: a log copied once goes stale while the user is looking at it. */
async function openLog(event: Event): Promise<void> {
  if (!(event.target as HTMLDetailsElement).open)
    return
  error.value = undefined
  const answer = await kernel.call<string[]>('diag_logs_tail', { lines: 200 })
  logAsked.value = true
  if (!answer.ok) {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  logLines.value = [...answer.value]
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

/** How often the recorder is re-read while this page is open. */
const METRICS_POLL_MS = 5_000

/** The last metric snapshot, if one has been read. `undefined` means "not asked yet", which is not "empty". */
const metrics = ref<MetricsSnapshot | undefined>(undefined)

/** Whether the Refresh button's read is in flight. Polling does not set it: the button is not what is happening. */
const refreshing = ref(false)

/**
 * Reads the recorder's snapshot.
 *
 * It does **not** clear `error` first: a poll is not a user action, and clearing the notice every few seconds would make
 * a refusal disappear while its cause is still there.
 */
async function loadMetrics(): Promise<void> {
  const answer = await kernel.call<MetricsSnapshot>('diag_metrics')
  if (!answer.ok) {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  metrics.value = answer.value
}

/** Reads it now, for the button. */
async function refreshMetrics(): Promise<void> {
  refreshing.value = true
  await loadMetrics()
  refreshing.value = false
}

/**
 * The key as the kernel renders it (`SeriesKey::render`).
 *
 * ★ The same spelling as a log line or a diagnostic export, rather than a prettier one: two spellings for one series is how
 * "the metric in the report" and "the metric in the panel" become two different things to look up.
 */
function seriesKey(key: SeriesKey): string {
  if (key.labels.length === 0)
    return key.name
  return `${key.name}{${key.labels.map(([name, value]) => `${name}=${value}`).join(',')}}`
}

/** One value as a short string; a histogram says how many observations and what they averaged. */
function seriesValue(value: MetricValue): string {
  if (value.kind !== 'histogram')
    return String(value.value)
  // ★ No observations is not a mean of zero: 0/0 written as "0" would make a histogram that never ran look like 0 ms.
  const mean = value.value.count === 0 ? '—' : (value.value.sum / value.value.count).toFixed(2)
  return messages.settings.metrics.histogram(value.value.count, mean)
}

let metricsTimer: ReturnType<typeof setInterval> | undefined

onMounted(async () => {
  await loadMetrics()
  metricsTimer = setInterval(() => {
    void loadMetrics()
  }, METRICS_POLL_MS)
})

onUnmounted(() => {
  if (metricsTimer !== undefined)
    clearInterval(metricsTimer)
})

onMounted(load)

/** The notice's Retry: everything this page read, not only the two answers the first mount asked for. */
async function reload(): Promise<void> {
  await Promise.all([load(), loadMetrics()])
}

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
