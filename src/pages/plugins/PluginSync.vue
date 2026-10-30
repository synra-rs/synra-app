<template>
  <AButton ref="trigger" size="small" :loading="busy" :disabled="operations.busy.has(plugin.pluginId)" @click="show">
    {{ messages.plugins.syncAction }}
  </AButton>
  <AModal
    :open="opened"
    :title="messages.plugins.sync.title(plugin.title)"
    :get-container="false"
    :footer="null"
    :closable="!sending"
    :mask-closable="!sending"
    :keyboard="!sending"
    @cancel="close"
  >
    <div class="min-w-0 stack gap-4 [&_.ant-btn]:min-h-11">
      <p v-if="busy" role="status" class="m-0 text-sm text-ink-dim">
        {{ sending ? messages.plugins.syncing : messages.plugins.sync.planning }}
      </p>
      <ErrorNotice v-if="error" :code="error.code" :actions="error.actions" @retry="readPlan" />
      <p v-if="stale" class="m-0 text-sm text-ink-dim">
        {{ messages.plugins.sync.stale }}
      </p>
      <ul v-if="plan?.targets.length" class="m-0 stack list-none gap-3 p-0" data-sync-targets>
        <li v-for="target in plan.targets" :key="target.deviceId" class="min-w-0 stack gap-1">
          <div class="min-w-0 row-between flex-wrap gap-2 text-sm">
            <span class="min-w-0 break-all">{{ target.displayName || target.deviceId }}</span>
            <span class="text-xs text-ink-dim">{{ target.online ? messages.plugins.sync.online : messages.plugins.sync.offline }}</span>
          </div>
          <p class="m-0 break-all text-xs text-ink-dim">
            {{ target.needsArtifact ? messages.plugins.sync.needed : messages.plugins.sync.current }}
            <span v-if="target.version" class="block">{{ messages.plugins.version }} {{ target.version }}</span>
          </p>
        </li>
      </ul>
      <p v-if="plan && !plan.targets.length" class="m-0 text-sm text-ink-dim">
        {{ messages.plugins.sync.noDevices }}
      </p>
      <p v-else-if="plan && !plan.targets.some(target => target.needsArtifact)" class="m-0 text-sm text-ink-dim">
        {{ messages.plugins.syncNothing }}
      </p>
      <p v-else-if="plan && !plan.targets.some(target => target.needsArtifact && target.online) && !report" class="m-0 text-sm text-ink-dim">
        {{ messages.plugins.sync.noneOnline }}
      </p>
      <div v-if="report" class="stack gap-2" role="status" data-sync-report>
        <p class="m-0 text-sm">
          {{ messages.plugins.syncDone(report.updated, report.unchanged, report.failures?.length ?? 0) }}
        </p>
        <ul v-if="report.failures?.length" class="m-0 stack list-none gap-3 p-0">
          <li v-for="failure in report.failures" :key="failure.deviceId" class="min-w-0 stack gap-1">
            <span class="break-all text-sm">{{ deviceName(failure.deviceId) }}</span>
            <ErrorNotice :code="failureOf(failure.error).code" />
            <details class="text-xs text-ink-dim">
              <summary class="min-h-11 cursor-pointer py-3">
                {{ messages.plugins.sync.failureDetails }}
              </summary>
              <p class="m-0 break-all font-mono">
                {{ failure.deviceId }}
              </p>
              <pre class="m-0 whitespace-pre-wrap break-all">{{ detailOf(failure.error) }}</pre>
            </details>
          </li>
        </ul>
      </div>
      <div class="row flex-wrap justify-end gap-2">
        <AButton ref="closeButton" :disabled="sending" @click="close">
          {{ messages.plugins.sync.close }}
        </AButton>
        <AButton :disabled="operations.busy.has(plugin.pluginId)" @click="readPlan">
          {{ messages.plugins.sync.refresh }}
        </AButton>
        <AButton v-if="plan && !report" type="primary" :loading="sending" :disabled="!canSend || operations.busy.has(plugin.pluginId)" @click="send">
          {{ messages.plugins.sync.start }}
        </AButton>
      </div>
    </div>
  </AModal>
</template>

<script setup lang="ts">
import type { PluginSnapshot, SyncPlanDto, SyncReport } from '@synra/protocol'
import type { Failure } from './actions'
import type { usePluginOperations } from './operations'
import { Button as AButton, Modal as AModal } from 'ant-design-vue'
import { computed, nextTick, onScopeDispose, ref, watch } from 'vue'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { kernel } from '../../kernel'
import { messages } from '../../messages'
import { planSync, runSync } from './actions'

const props = defineProps<{ plugin: PluginSnapshot, operations: ReturnType<typeof usePluginOperations> }>()
const opened = ref(false)
const plan = ref<SyncPlanDto>()
const report = ref<SyncReport>()
const error = ref<Failure>()
const stale = ref(false)
const trigger = ref<{ $el: HTMLButtonElement }>()
const closeButton = ref<{ $el: HTMLButtonElement }>()
const sending = computed(() => props.operations.busy.get(props.plugin.pluginId) === 'syncRun')
const busy = computed(() => sending.value || props.operations.busy.get(props.plugin.pluginId) === 'syncPlan')
const canSend = computed(() => !stale.value && plan.value?.targets.some(target => target.needsArtifact && target.online))
let revision = 0
let alive = true

watch(() => [props.plugin.pluginId, props.plugin.version], () => {
  if (!sending.value) {
    revision++
    plan.value = undefined
    stale.value = opened.value
  }
})
onScopeDispose(() => {
  alive = false
  revision++
})

async function show(): Promise<void> {
  if (props.operations.busy.has(props.plugin.pluginId))
    return
  opened.value = true
  await nextTick()
  closeButton.value?.$el.focus()
  await readPlan()
}

async function close(): Promise<void> {
  if (sending.value)
    return
  revision++
  opened.value = false
  await nextTick()
  trigger.value?.$el.focus()
}

async function readPlan(): Promise<void> {
  if (!opened.value || props.operations.busy.has(props.plugin.pluginId))
    return
  const current = ++revision
  error.value = undefined
  const outcome = await props.operations.run(props.plugin.pluginId, 'syncPlan', () => planSync(kernel, props.plugin.pluginId))
  if (!alive || current !== revision || !opened.value || !outcome)
    return
  if (outcome.ok) {
    plan.value = outcome.value
    report.value = undefined
    stale.value = false
  }
  else {
    error.value = outcome.failure
    stale.value = true
  }
}

async function send(): Promise<void> {
  const chosen = plan.value
  if (!chosen || !canSend.value || report.value || props.operations.busy.has(props.plugin.pluginId))
    return
  const current = ++revision
  error.value = undefined
  const outcome = await props.operations.run(props.plugin.pluginId, 'syncRun', () => runSync(kernel, chosen))
  if (!alive || current !== revision || !outcome)
    return
  if (outcome.ok) {
    report.value = outcome.value
  }
  else {
    error.value = outcome.failure
    stale.value = true
  }
  await nextTick()
  closeButton.value?.$el.focus()
}

function deviceName(id: string): string {
  return plan.value?.targets.find(target => target.deviceId === id)?.displayName || id
}

function failureOf(value: unknown): Failure {
  const record = value !== null && typeof value === 'object' ? value as Record<string, unknown> : {}
  return { code: typeof record.code === 'string' ? record.code : 'unknown', message: typeof record.message === 'string' ? record.message : 'error.unknown' }
}

function detailOf(value: unknown): string {
  return typeof value === 'string' ? value : JSON.stringify(value, null, 2) ?? ''
}
</script>
