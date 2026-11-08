import type { CrashReport, DiagExportResult, IpcError, MetricsSnapshot, MetricValue, ProbeReportDto, SeriesKey } from '@synra/protocol'
import type { Ref } from 'vue'
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { kernel } from '../../app/kernel'
import { messages } from '../../messages'

export function useDiagnostics(active: Ref<boolean>) {
  const openPanels = ref<string[]>([])
  const visible = ref(document.visibilityState !== 'hidden')
  const errors = reactive<Partial<Record<'metrics' | 'probes' | 'logs' | 'crashes' | 'export', IpcError | undefined>>>({})
  const metrics = ref<MetricsSnapshot>()
  const metricsAt = ref<number>()
  const refreshing = ref(false)
  const metricsEnabled = computed(() => active.value && openPanels.value.includes('metrics') && visible.value)
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
    return status === 'ok' ? 'green' : status === 'warn' ? 'orange' : status === 'fail' ? 'red' : 'default'
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
    if (loadingLogs.value || !active.value || !openPanels.value.includes('logs'))
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
  watch(() => active.value && openPanels.value.includes('logs'), (open) => {
    logGeneration++
    if (open)
      void loadLogs()
  })
  watch(() => active.value && openPanels.value.includes('export'), (open) => {
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
  return { openPanels, errors, metrics, metricsAt, refreshing, metricsEnabled, loadMetrics, startup, seriesKey, seriesValue, exportAudit, exportDebug, exportLines, exporting, exported, exportDiagnostics, confirmDismiss, crashesDismissed, dismissing, crashes, loadingCrashes, dismissCrashes, showCrashes, probeReport, probing, onlyProblems, probesSummary, visibleProbes, probeColor, runProbes, logLines, logAsked, loadingLogs, loadLogs }
}
