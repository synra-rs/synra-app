<template>
  <div class="min-w-0 stack gap-5" data-testid="settings-diagnostics">
    <ACollapse v-model:active-key="openPanels" ghost>
      <ACollapse.Panel key="metrics" :header="messages.settings.metrics.title">
        <div class="min-w-0 stack gap-3">
          <div class="row-between flex-wrap gap-2">
            <span class="text-xs text-ink-dim">{{ messages.settings.metrics.detail }}</span>
            <AButton :loading="refreshing" :disabled="!metricsEnabled" @click="loadMetrics">
              {{ messages.settings.metrics.refresh }}
            </AButton>
          </div>
          <SettingsFeedback :error="errors.metrics" />
          <p v-if="metricsAt" class="m-0 text-xs text-ink-dim" role="status">
            {{ messages.settings.metrics.updated(formatDateTime(metricsAt)) }} · {{ metricsEnabled && !errors.metrics ? messages.settings.metrics.live : messages.settings.metrics.paused }}
          </p>
          <p v-if="metrics && metrics.overflowedTotal > 0" class="m-0 text-xs text-ink-dim">
            {{ messages.settings.metrics.overflow(metrics.overflowedTotal, metrics.maxSeriesPerMetric) }}
          </p>
          <div v-if="startup" class="row flex-wrap items-baseline gap-2 text-xs">
            <span class="text-ink-dim">{{ messages.settings.metrics.startup }}</span>
            <span class="font-mono">{{ startup.ms.toFixed(0) }} ms</span>
            <span :class="startup.within ? 'text-ink-dim' : 'text-error'">
              {{ startup.within ? messages.settings.metrics.startupWithin : messages.settings.metrics.startupOver }}
            </span>
          </div>
          <AEmpty v-if="metrics && metrics.values.length === 0" :description="messages.settings.metrics.empty" />
          <AList v-else-if="metrics" size="small">
            <AList.Item v-for="series in metrics.values" :key="seriesKey(series.key)" :style="{ paddingInline: 0 }">
              <div class="row-between gap-3">
                <span class="min-w-0 truncate text-xs font-mono">{{ seriesKey(series.key) }}</span>
                <span class="text-sm font-mono">{{ seriesValue(series.value) }}</span>
              </div>
            </AList.Item>
          </AList>
        </div>
      </ACollapse.Panel>
      <ACollapse.Panel key="probes" :header="messages.settings.diagnostics.probesTitle">
        <SettingsFeedback :error="errors.probes" />
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
      </ACollapse.Panel>
      <ACollapse.Panel key="logs" :header="messages.settings.diagnostics.logs">
        <div class="stack gap-3">
          <AButton :loading="loadingLogs" @click="loadLogs">
            {{ messages.settings.metrics.refresh }}
          </AButton>
          <SettingsFeedback :error="errors.logs" />
          <pre v-if="logLines.length" class="m-0 max-h-64 overflow-auto whitespace-pre-wrap break-all text-xs font-mono">{{ logLines.join('\n') }}</pre>
          <p v-else-if="logAsked" class="m-0 text-xs text-ink-dim">
            {{ messages.settings.diagnostics.logsEmpty }}
          </p>
        </div>
      </ACollapse.Panel>
      <ACollapse.Panel key="crashes" :header="messages.settings.diagnostics.crashesTitle">
        <SettingsFeedback :error="errors.crashes" />
        <div class="stack gap-2">
          <p class="m-0 text-xs text-ink-dim">
            {{ messages.settings.diagnostics.dismissDetail }}
          </p>
          <div class="row items-center gap-2">
            <AButton danger :loading="dismissing" :disabled="loadingCrashes" @click="confirmDismiss = true">
              {{ messages.settings.diagnostics.dismissCrashes }}
            </AButton>
            <AButton :loading="loadingCrashes" :disabled="dismissing" @click="showCrashes">
              {{ messages.settings.diagnostics.showCrashes }}
            </AButton>
            <span v-if="crashesDismissed" class="text-xs text-ink-dim">
              {{ messages.settings.diagnostics.dismissed }}
            </span>
          </div>
          <ul v-if="crashes" class="m-0 stack gap-2 p-0">
            <li v-if="crashes.length === 0" class="text-xs text-ink-dim">
              {{ messages.settings.diagnostics.crashesEmpty }}
            </li>
            <li
              v-for="report in crashes"
              :key="`${report.atMs}-${report.location ?? ''}`"
              class="stack gap-1 text-xs"
            >
              <span class="text-ink-dim font-mono">
                {{ formatDateTime(report.atMs) }} · {{ report.version }}
              </span>
              <span>{{ report.message ?? messages.settings.diagnostics.crashNoMessage }}</span>
              <span v-if="report.location" class="text-ink-dim font-mono">{{ report.location }}</span>
            </li>
          </ul>
        </div>
      </ACollapse.Panel>
      <ACollapse.Panel key="export" :header="messages.settings.diagnostics.export">
        <div class="stack gap-3">
          <p class="m-0 text-xs text-ink-dim">
            {{ messages.settings.diagnostics.detail }}
          </p>
          <SettingsFeedback :error="errors.export" />
          <div class="stack gap-1">
            <ACheckbox
              :checked="exportAudit" :disabled="exporting"
              @update:checked="(checked) => (exportAudit = checked)"
            >
              {{ messages.settings.diagnostics.audit }}
            </ACheckbox>
            <span class="text-xs text-ink-dim">{{ messages.settings.diagnostics.auditDetail }}</span>
          </div>
          <div class="stack gap-1">
            <ACheckbox
              :checked="exportDebug" :disabled="exporting"
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
        </div>
      </ACollapse.Panel>
    </ACollapse>
    <AModal :open="confirmDismiss" :title="messages.settings.diagnostics.dismissCrashes" :ok-text="messages.settings.diagnostics.dismissCrashes" :cancel-text="messages.settings.field.cancel" :confirm-loading="dismissing" :closable="!dismissing" :mask-closable="!dismissing" :keyboard="!dismissing" @ok="dismissCrashes" @cancel="confirmDismiss = false">
      <p>{{ messages.settings.diagnostics.dismissDetail }}</p>
      <SettingsFeedback :error="errors.crashes" />
    </AModal>
  </div>
</template>

<script setup lang="ts">
import type { CrashReport, DiagExportResult, IpcError, MetricsSnapshot, MetricValue, ProbeReportDto, SeriesKey } from '@synra/protocol'
import { Button as AButton, Checkbox as ACheckbox, Collapse as ACollapse, Empty as AEmpty, InputNumber as AInputNumber, List as AList, Modal as AModal, Tag as ATag } from 'ant-design-vue'
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { kernel } from '../../kernel'
import { formatDateTime } from '../../locale-format'
import { messages } from '../../messages'
import SettingsFeedback from './SettingsFeedback.vue'

const props = defineProps<{ active: boolean }>()
const openPanels = ref<string[]>([])
const visible = ref(document.visibilityState !== 'hidden')
const errors = reactive<Partial<Record<'metrics' | 'probes' | 'logs' | 'crashes' | 'export', IpcError | undefined>>>({})
const metrics = ref<MetricsSnapshot>()
const metricsAt = ref<number>()
const refreshing = ref(false)
const metricsEnabled = computed(() => props.active && openPanels.value.includes('metrics') && visible.value)
let generation = 0
let timer: ReturnType<typeof setTimeout> | undefined
let pendingMetrics = false
let resumeMetrics = false
let disposed = false

function stopTimer(): void {
  if (timer !== undefined)
    clearTimeout(timer)
  timer = undefined
}

async function loadMetrics(): Promise<void> {
  if (!metricsEnabled.value || pendingMetrics || disposed)
    return
  stopTimer()
  const ticket = generation
  pendingMetrics = true
  refreshing.value = true
  try {
    const answer = await kernel.call<MetricsSnapshot>('diag_metrics')
    if (disposed || ticket !== generation || !metricsEnabled.value)
      return
    if (!answer.ok) {
      errors.metrics = answer.error
      return
    }
    errors.metrics = undefined
    metrics.value = answer.value
    metricsAt.value = Date.now()
    timer = setTimeout(() => {
      void loadMetrics()
    }, 5_000)
  }
  finally {
    pendingMetrics = false
    refreshing.value = false
    if (resumeMetrics && metricsEnabled.value && !disposed) {
      resumeMetrics = false
      void loadMetrics()
    }
  }
}

watch(metricsEnabled, (enabled) => {
  generation++
  stopTimer()
  resumeMetrics = enabled && pendingMetrics
  if (enabled && !pendingMetrics)
    void loadMetrics()
}, { flush: 'sync' })

const startup = computed(() => {
  const series = metrics.value?.values.find(sample => sample.key.name === 'startup_duration_ms'
    && sample.key.labels.some(([name, value]) => name === 'phase' && value === 'total'))
  if (!series || series.value.kind !== 'histogram' || series.value.value.count === 0)
    return undefined
  const ms = series.value.value.sum / series.value.value.count
  return { ms, within: ms <= 500 }
})
function seriesKey(key: SeriesKey): string {
  return key.labels.length ? `${key.name}{${key.labels.map(([name, value]) => `${name}=${value}`).join(',')}}` : key.name
}
function seriesValue(value: MetricValue): string {
  if (value.kind !== 'histogram')
    return String(value.value)
  return messages.settings.metrics.histogram(value.value.count, value.value.count === 0 ? '—' : (value.value.sum / value.value.count).toFixed(2))
}

const exportAudit = ref(false)
const exportDebug = ref(false)
const exportLines = ref(200)
const exporting = ref(false)
const exported = ref<DiagExportResult>()
async function exportDiagnostics(): Promise<void> {
  if (exporting.value)
    return
  exporting.value = true
  errors.export = undefined
  exported.value = undefined
  const answer = await kernel.call<DiagExportResult>('diag_export', {
    options: { includeAudit: exportAudit.value, includeDebugLogs: exportDebug.value, ...(exportDebug.value ? { logLines: exportLines.value } : {}) }
  })
  exporting.value = false
  if (disposed)
    return
  if (answer.ok)
    exported.value = answer.value
  else
    errors.export = answer.error
}

const confirmDismiss = ref(false)
const crashesDismissed = ref(false)
const dismissing = ref(false)
const crashes = ref<readonly CrashReport[]>()
const loadingCrashes = ref(false)
let crashGeneration = 0
async function dismissCrashes(): Promise<void> {
  if (dismissing.value || loadingCrashes.value)
    return
  dismissing.value = true
  errors.crashes = undefined
  const answer = await kernel.call<null>('diag_crash_dismiss')
  dismissing.value = false
  if (disposed)
    return
  if (!answer.ok) {
    errors.crashes = answer.error
    return
  }
  crashGeneration++
  crashesDismissed.value = true
  crashes.value = []
  confirmDismiss.value = false
}
async function showCrashes(): Promise<void> {
  if (loadingCrashes.value || dismissing.value)
    return
  loadingCrashes.value = true
  errors.crashes = undefined
  const ticket = crashGeneration
  const answer = await kernel.call<readonly CrashReport[]>('diag_crashes')
  loadingCrashes.value = false
  if (disposed || ticket !== crashGeneration)
    return
  if (answer.ok)
    crashes.value = answer.value
  else
    errors.crashes = answer.error
}

const probeReport = ref<ProbeReportDto>()
const probing = ref(false)
const onlyProblems = ref(true)
const probesSummary = computed(() => {
  const count = (status: string): number => (probeReport.value?.results ?? []).filter(result => result.status === status).length
  return messages.settings.diagnostics.probesSummary(count('ok'), count('warn'), count('fail'))
})
const visibleProbes = computed(() => (probeReport.value?.results ?? []).filter(result => !onlyProblems.value || result.status !== 'ok'))
function probeColor(status: string): string {
  return status === 'ok' ? 'green' : status === 'warn' ? 'orange' : 'red'
}
async function runProbes(): Promise<void> {
  if (probing.value)
    return
  probing.value = true
  errors.probes = undefined
  const answer = await kernel.call<ProbeReportDto>('diag_probes')
  probing.value = false
  if (disposed)
    return
  if (answer.ok)
    probeReport.value = answer.value
  else
    errors.probes = answer.error
}

const logLines = ref<readonly string[]>([])
const logAsked = ref(false)
const loadingLogs = ref(false)
let logGeneration = 0
async function loadLogs(): Promise<void> {
  if (loadingLogs.value || !props.active || !openPanels.value.includes('logs'))
    return
  loadingLogs.value = true
  errors.logs = undefined
  const ticket = logGeneration
  const answer = await kernel.call<string[]>('diag_logs_tail', { lines: 200 })
  loadingLogs.value = false
  if (disposed || ticket !== logGeneration)
    return
  if (answer.ok) {
    logLines.value = answer.value
    logAsked.value = true
  }
  else {
    errors.logs = answer.error
  }
}
watch(() => props.active && openPanels.value.includes('logs'), (open) => {
  logGeneration++
  if (open)
    void loadLogs()
})
watch(() => props.active && openPanels.value.includes('export'), (open) => {
  if (open && !exporting.value) {
    exportAudit.value = false
    exportDebug.value = false
  }
})

function updateVisibility(): void {
  visible.value = document.visibilityState !== 'hidden'
}
onMounted(() => document.addEventListener('visibilitychange', updateVisibility))
onUnmounted(() => {
  disposed = true
  generation++
  logGeneration++
  stopTimer()
  document.removeEventListener('visibilitychange', updateVisibility)
})
</script>
