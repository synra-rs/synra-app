<template>
  <div class="min-w-0 stack gap-5" data-testid="settings-diagnostics">
    <ACollapse v-model:active-key="openPanels" ghost>
      <template #expandIcon="panel">
        <CollapseArrow :panel="panel" />
      </template>
      <ACollapse.Panel key="device" :header="`设备信息 · ${deviceDetails[0]?.value ?? ''}`" force-render>
        <div class="stack gap-2 text-sm">
          <p class="m-0 text-ink-dim">{{ messages.settings.diagnostics.deviceDetail }}</p>
          <AList size="small" bordered>
            <AList.Item v-for="item in deviceDetails" :key="item.label">
              <span class="text-ink-dim">{{ item.label }}</span>
              <span class="ms-auto max-w-[65%] break-all text-end font-mono">{{ item.value }}</span>
            </AList.Item>
          </AList>
        </div>
      </ACollapse.Panel>
      <ACollapse.Panel key="metrics" :header="messages.settings.metrics.title">
        <div class="min-w-0 stack gap-3">
          <div class="row-between flex-wrap gap-2">
            <span class="text-sm text-ink-dim">{{ messages.settings.metrics.detail }}</span>
            <AButton :loading="refreshing" :disabled="!metricsEnabled" @click="loadMetrics">
              {{ messages.settings.metrics.refresh }}
            </AButton>
          </div>
          <SettingsFeedback :error="errors.metrics" />
          <p v-if="metricsAt" class="m-0 text-sm text-ink-dim" role="status">
            {{ messages.settings.metrics.updated(formatDateTime(metricsAt)) }} · {{ metricsEnabled && !errors.metrics ? messages.settings.metrics.live : messages.settings.metrics.paused }}
          </p>
          <p v-if="metrics && metrics.overflowedTotal > 0" class="m-0 text-sm text-ink-dim">
            {{ messages.settings.metrics.overflow(metrics.overflowedTotal, metrics.maxSeriesPerMetric) }}
          </p>
          <div v-if="startup" class="row flex-wrap items-baseline gap-2 text-sm">
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
                <span class="min-w-0 truncate text-sm font-mono">{{ seriesKey(series.key) }}</span>
                <span class="text-base font-mono">{{ seriesValue(series.value) }}</span>
              </div>
            </AList.Item>
          </AList>
        </div>
      </ACollapse.Panel>
      <ACollapse.Panel key="probes" :header="messages.settings.diagnostics.probesTitle">
        <SettingsFeedback :error="errors.probes" />
        <div class="stack gap-2">
          <p class="m-0 text-sm text-ink-dim">
            {{ messages.settings.diagnostics.probesDetail }}
          </p>
          <div class="row items-center gap-2">
            <AButton :loading="probing" @click="runProbes">
              {{ messages.settings.diagnostics.probes }}
            </AButton>
            <span v-if="probeReport" class="text-sm text-ink-dim">{{ probesSummary }}</span>
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
                <span class="text-sm font-mono">{{ result.name }}</span>
              </div>
              <span class="text-sm text-ink-dim">{{ result.detail }}</span>
              <span v-if="result.suggestion" class="text-sm">{{ result.suggestion }}</span>
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
          <pre v-if="logLines.length" class="m-0 max-h-64 overflow-auto whitespace-pre-wrap break-all text-sm font-mono">{{ logLines.join('\n') }}</pre>
          <p v-else-if="logAsked" class="m-0 text-sm text-ink-dim">
            {{ messages.settings.diagnostics.logsEmpty }}
          </p>
        </div>
      </ACollapse.Panel>
      <ACollapse.Panel key="crashes" :header="messages.settings.diagnostics.crashesTitle">
        <SettingsFeedback :error="errors.crashes" />
        <div class="stack gap-2">
          <p class="m-0 text-sm text-ink-dim">
            {{ messages.settings.diagnostics.dismissDetail }}
          </p>
          <div class="row items-center gap-2">
            <AButton danger :loading="dismissing" :disabled="loadingCrashes" @click="confirmDismiss = true">
              {{ messages.settings.diagnostics.dismissCrashes }}
            </AButton>
            <AButton :loading="loadingCrashes" :disabled="dismissing" @click="showCrashes">
              {{ messages.settings.diagnostics.showCrashes }}
            </AButton>
            <span v-if="crashesDismissed" class="text-sm text-ink-dim">
              {{ messages.settings.diagnostics.dismissed }}
            </span>
          </div>
          <ul v-if="crashes" class="m-0 stack gap-2 p-0">
            <li v-if="crashes.length === 0" class="text-sm text-ink-dim">
              {{ messages.settings.diagnostics.crashesEmpty }}
            </li>
            <li
              v-for="report in crashes"
              :key="`${report.atMs}-${report.location ?? ''}`"
              class="stack gap-1 text-sm"
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
          <p class="m-0 text-sm text-ink-dim">
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
            <span class="text-sm text-ink-dim">{{ messages.settings.diagnostics.auditDetail }}</span>
          </div>
          <div class="stack gap-1">
            <ACheckbox
              :checked="exportDebug" :disabled="exporting"
              @update:checked="(checked) => (exportDebug = checked)"
            >
              {{ messages.settings.diagnostics.debug }}
            </ACheckbox>
            <span class="text-sm text-ink-dim">{{ messages.settings.diagnostics.debugDetail }}</span>
          </div>
          <div v-if="exportDebug" class="row gap-2">
            <span class="text-base">{{ messages.settings.diagnostics.lines }}</span>
            <AInputNumber
              :value="exportLines"
              :min="1"
              :max="10000"
              :precision="0"
              :aria-label="messages.settings.diagnostics.lines"
              @update:value="(value) => (exportLines = typeof value === 'number' ? value : 1)"
            />
          </div>
          <div class="row gap-2">
            <AButton :loading="exporting" @click="exportDiagnostics">
              {{ messages.settings.diagnostics.export }}
            </AButton>
          </div>
          <div v-if="exported" class="stack gap-1">
            <span class="text-sm text-ink-dim font-mono">{{ exported.path }}</span>
            <span class="text-sm">{{ messages.settings.diagnostics.included(exported.included.join(', ')) }}</span>
            <span v-if="exported.excluded.length > 0" class="text-sm text-ink-dim">
              {{ messages.settings.diagnostics.excluded(exported.excluded.join('; ')) }}
            </span>
          </div>
        </div>
      </ACollapse.Panel>
    </ACollapse>
    <AModal :open="confirmVisible" :title="messages.settings.diagnostics.dismissCrashes" :ok-text="messages.settings.diagnostics.dismissCrashes" :cancel-text="messages.settings.field.cancel" :confirm-loading="dismissing" :closable="!dismissing" :mask-closable="!dismissing" :keyboard="!dismissing" @ok="dismissCrashes" @cancel="confirmDismiss = false">
      <p>{{ messages.settings.diagnostics.dismissDetail }}</p>
      <SettingsFeedback :error="errors.crashes" />
    </AModal>
  </div>
</template>

<script setup lang="ts">
import { Button as AButton, Checkbox as ACheckbox, Collapse as ACollapse, Empty as AEmpty, InputNumber as AInputNumber, List as AList, Modal as AModal, Tag as ATag } from 'ant-design-vue'
import { computed, toRef } from 'vue'
import { messages } from '../../messages'
import { useDialogVisibility } from '../../stores/dialogs'
import { formatDateTime } from '../../utils/locale-format'
import CollapseArrow from '../CollapseArrow.vue'
import SettingsFeedback from './SettingsFeedback.vue'
import { useDiagnostics } from './use-diagnostics'

const props = defineProps<{ active: boolean }>()
const { openPanels, errors, metrics, metricsAt, refreshing, metricsEnabled, loadMetrics, startup, seriesKey, seriesValue, exportAudit, exportDebug, exportLines, exporting, exported, exportDiagnostics, confirmDismiss, crashesDismissed, dismissing, crashes, loadingCrashes, dismissCrashes, showCrashes, probeReport, probing, onlyProblems, probesSummary, visibleProbes, probeColor, runProbes, logLines, logAsked, loadingLogs, loadLogs } = useDiagnostics(toRef(props, 'active'))
const deviceDetails = computed(() => {
  const nav = navigator
  const screen = window.screen
  return [
    { label: 'User agent', value: nav.userAgent },
    { label: 'Platform', value: nav.platform || '—' },
    { label: 'Language', value: nav.language || '—' },
    { label: 'Viewport', value: `${window.innerWidth} × ${window.innerHeight}` },
    { label: 'Screen', value: `${screen.width} × ${screen.height} @ ${window.devicePixelRatio}x` },
    { label: 'Touch points', value: String(nav.maxTouchPoints) },
    { label: 'Tauri runtime', value: '__TAURI_INTERNALS__' in window ? 'yes' : 'no' },
    { label: 'Online', value: navigator.onLine ? 'yes' : 'no' }
  ]
})
const confirmVisible = useDialogVisibility(confirmDismiss, 60)
</script>
