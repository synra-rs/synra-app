<template>
  <!--
    A workspace page, but a narrow one: paired and nearby devices are two short lists whose rows carry a name, a
    platform and a couple of actions. At the shell's full 1200px those actions drifted ~700px from the name they belong
    to, which is the "controls pulled apart across a wide window" [03 §3](../../design-docs/ui-design/03-shell-and-navigation.md)
    warns against. The workspace width is therefore capped at 896px — wider than the reading measure, because the rows
    hold a name and an action set on one line, and narrower than a full workspace, because nothing here fills it.
  -->
  <section class="mx-auto max-w-4xl min-w-0 pb-4 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8" data-page-width="narrow-workspace">
    <div class="stack gap-6">
      <PageHeader :title="messages.devices.title">
        <AButton v-if="scanState" :loading="cancellingScan" :disabled="cancellingScan" @click="cancelScan">
          {{ messages.devices.cancelScan }}
        </AButton>
        <AButton v-else :loading="startingScan" :disabled="scanning" @click="scan">
          {{ startingScan ? messages.devices.scanning : messages.devices.scan }}
        </AButton>
        <AButton ref="pairTrigger" type="primary" :aria-expanded="showPairing" aria-controls="device-pairing" @click="showPairing = !showPairing">
          {{ pairing.active ? messages.devices.pairingResume : messages.devices.pairingTitle }}
        </AButton>
      </PageHeader>

      <div v-if="snapshotError" class="stack gap-2">
        <ErrorNotice :code="snapshotError.code" :message="snapshotError.message" :actions="snapshotError.actions?.filter(action => action.kind !== 'retry')" />
        <p v-if="devices.cards.length" class="m-0 text-xs text-ink-dim">
          {{ messages.devices.knownOnly }}
        </p>
        <div>
          <AButton :loading="refreshing" :disabled="refreshing" @click="refresh">
            {{ messages.devices.refresh }}
          </AButton>
        </div>
      </div>
      <p v-if="notice" class="m-0 text-sm" role="status">
        {{ notice }}
      </p>

      <div v-show="showPairing" id="device-pairing">
        <PairingPanel :selected="selectedPeer" :visible="showPairing" @select="selectedDeviceId = $event" />
      </div>

      <div v-if="scanning || scanReport || scanError" class="stack gap-2" aria-live="polite">
        <div v-if="scanError" class="stack gap-2">
          <ErrorNotice :code="scanError.code" :message="scanError.message" :actions="scanError.actions?.filter(action => action.kind !== 'retry')" />
          <p v-if="devices.cards.length" class="m-0 text-xs text-ink-dim">
            {{ messages.devices.scanFailedKnown }}
          </p>
          <div v-if="scanError.retryable">
            <AButton :disabled="cancellingScan || (!failedCancelId && scanning)" @click="retryScan">
              {{ messages.actions.retry }}
            </AButton>
          </div>
        </div>
        <p v-else class="m-0 row flex-wrap gap-2 text-sm text-ink-dim">
          <span v-if="scanning" class="i-carbon-circle-dash animate-spin" aria-hidden="true" />
          <span>{{ scanSummary }}</span>
          <span v-if="scanState">{{ messages.devices.scanProgress }} {{ scanState.found }}</span>
        </p>
        <details v-if="scanState" class="text-xs text-ink-dim">
          <summary class="min-h-11 row cursor-pointer md:min-h-8">
            {{ messages.devices.scanDetails }}
          </summary>
          <dl class="grid m-0 gap-2 py-2">
            <dt>{{ messages.devices.scanId }}</dt><dd class="m-0 break-all font-mono">
              {{ scanState.scanId }}
            </dd>
            <dt>{{ messages.devices.scanElapsed }}</dt><dd class="m-0">
              {{ scanState.elapsedMs }} ms
            </dd>
            <dt>{{ messages.devices.scanTotal }}</dt><dd class="m-0">
              {{ scanState.totalMs }} ms
            </dd>
          </dl>
        </details>
      </div>

      <p v-if="refreshing && !ready && !devices.cards.length" class="m-0 text-sm text-ink-dim" role="status">
        {{ messages.devices.loading }}
      </p>
      <AEmpty v-else-if="ready && !snapshotError && !devices.cards.length && !scanning" :description="scanReport === 'finished' ? messages.devices.scanEmpty : messages.devices.empty">
        <template #image>
          <span class="i-carbon-devices text-xl text-ink-dim" aria-hidden="true" />
        </template>
        <AButton @click="scan">
          {{ messages.devices.emptyAction }}
        </AButton>
      </AEmpty>

      <section v-if="devices.paired.length" :aria-label="messages.devices.paired">
        <h2 ref="pairedHeading" class="m-0 text-base font-medium" tabindex="-1">
          {{ messages.devices.paired }}
        </h2>
        <DeviceRow v-for="card in devices.paired" :key="card.deviceId" :card="card" :pairing-busy="pairing.active || pairing.busy" @forget="askForget" />
      </section>
      <section v-if="nearby.length" :aria-label="messages.devices.nearby">
        <h2 class="m-0 text-base font-medium">
          {{ messages.devices.nearby }}
        </h2>
        <DeviceRow v-for="card in nearby" :key="card.deviceId" :card="card" :pairing-busy="pairing.active || pairing.busy" @pair="pair" />
      </section>
    </div>

    <AModal v-if="forgetTarget" :open="true" :title="messages.devices.forgetTitle(forgetTarget.displayName)" :get-container="false" :footer="null" :closable="false" :mask-closable="!forgetting" :keyboard="!forgetting" @cancel="closeForget">
      <div class="stack gap-4">
        <p class="m-0 text-sm">
          {{ messages.devices.forgetHelp }}
        </p>
        <p class="m-0 break-all text-xs text-ink-dim font-mono">
          {{ forgetTarget.deviceId }}
        </p>
        <ErrorNotice v-if="forgetError" :code="forgetError.code" :message="forgetError.message" :actions="forgetError.actions?.filter(action => action.kind !== 'retry')" />
        <div class="row justify-end gap-2 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8">
          <AButton ref="keepButton" :disabled="forgetting" @click="closeForget">
            {{ messages.devices.forgetCancel }}
          </AButton>
          <AButton type="primary" danger :loading="forgetting" :disabled="forgetting || !devices.get(forgetTarget.deviceId)?.paired" @click="forget">
            {{ messages.devices.forget }}
          </AButton>
        </div>
      </div>
    </AModal>
  </section>
</template>

<script setup lang="ts">
import type { DeviceCard, DevicesSnapshot, IpcError, ScanOptions } from '@synra/protocol'
import { Button as AButton, Empty as AEmpty, Modal as AModal } from 'ant-design-vue'
import { computed, nextTick, onMounted, onScopeDispose, ref, watch } from 'vue'
import ErrorNotice from '../components/ErrorNotice.vue'
import PageHeader from '../components/PageHeader.vue'
import PairingPanel from '../components/PairingPanel.vue'
import { kernel } from '../kernel'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'
import { usePairingStore } from '../stores/pairing'
import DeviceRow from './devices/DeviceRow.vue'

const devices = useDevicesStore()
const pairing = usePairingStore()
const selectedDeviceId = ref<string>()
const selectedPeer = computed(() => selectedDeviceId.value ? devices.get(selectedDeviceId.value) : undefined)
const nearby = computed(() => devices.cards.filter(card => !card.paired))
const showPairing = ref(false)
watch(() => [pairing.active, pairing.error], ([active, failure]) => {
  if (active || failure)
    showPairing.value = true
}, { immediate: true })
function pair(deviceId: string): void {
  if (pairing.active || pairing.busy)
    return
  selectedDeviceId.value = deviceId
  showPairing.value = true
}

let disposed = false
let readRevision = 0
let reading: Promise<boolean> | undefined
const snapshotError = ref<IpcError>()
const refreshing = ref(false)
const ready = ref(devices.identity !== undefined)
function refresh(): Promise<boolean> {
  readRevision += 1
  if (reading)
    return reading
  refreshing.value = true
  reading = readLatest().finally(() => {
    reading = undefined
    refreshing.value = false
  })
  return reading
}
async function readLatest(): Promise<boolean> {
  while (true) {
    if (disposed)
      return false
    const revision = readRevision
    const answer = await kernel.call<DevicesSnapshot>('devices_snapshot')
    if (disposed)
      return false
    if (revision !== readRevision)
      continue
    snapshotError.value = answer.ok ? undefined : answer.error
    if (!answer.ok)
      return false
    devices.applySnapshot(answer.value)
    ready.value = true
    return true
  }
}

const NETWORK_SCAN: ScanOptions = { includePaired: false, cachedOnly: false }
const CACHED_SCAN: ScanOptions = { includePaired: false, cachedOnly: true }
const startingScan = ref(false)
const cancellingScan = ref(false)
const stopAccepted = ref(false)
const scanState = computed(() => devices.scan)
const scanning = computed(() => startingScan.value || devices.scanning || cancellingScan.value)
const scanReport = ref<'finished' | 'stopRequested' | 'cached'>()
const scanError = ref<IpcError>()
const failedCancelId = ref<string>()
let scanGeneration = 0
const scanSummary = computed(() => scanning.value
  ? stopAccepted.value ? messages.devices.scanStopping : messages.devices.scanning
  : scanReport.value === 'cached'
    ? messages.devices.scanCached
    : scanReport.value === 'stopRequested' ? messages.devices.scanStopped : messages.devices.scanFinished)

async function scan(): Promise<void> {
  if (scanning.value)
    return
  startingScan.value = true
  stopAccepted.value = false
  scanReport.value = undefined
  scanError.value = undefined
  failedCancelId.value = undefined
  try {
    const answer = await kernel.call<string>('devices_scan_start', { opts: NETWORK_SCAN })
    if (disposed)
      return
    if (!answer.ok) {
      if (answer.error.code !== 'unsupported') {
        scanError.value = answer.error
        return
      }
      const cached = await kernel.call<string>('devices_scan_start', { opts: CACHED_SCAN })
      if (disposed)
        return
      if (!cached.ok) {
        scanError.value = cached.error
        return
      }
      scanReport.value = 'cached'
    }
    const refreshed = await refresh()
    if (refreshed && !disposed && !devices.scanning && scanReport.value !== 'cached')
      scanReport.value = stopAccepted.value ? 'stopRequested' : 'finished'
  }
  finally {
    startingScan.value = false
  }
}

async function cancelScan(): Promise<void> {
  const id = scanState.value?.scanId
  if (!id || cancellingScan.value)
    return
  const generation = scanGeneration
  cancellingScan.value = true
  scanError.value = undefined
  failedCancelId.value = id
  try {
    const answer = await kernel.call<null>('devices_scan_cancel', { scanId: id })
    if (disposed)
      return
    if (!answer.ok) {
      scanError.value = answer.error
      return
    }
    stopAccepted.value = generation === scanGeneration && (!scanState.value || scanState.value.scanId === id)
    const refreshed = await refresh()
    if (refreshed && !disposed && !startingScan.value && !devices.scanning && stopAccepted.value)
      scanReport.value = 'stopRequested'
  }
  finally {
    cancellingScan.value = false
  }
}

async function retryScan(): Promise<void> {
  if (failedCancelId.value) {
    if (scanState.value?.scanId === failedCancelId.value)
      await cancelScan()
    else if (await refresh())
      scanError.value = undefined
  }
  else {
    await scan()
  }
}
watch(() => devices.scan?.scanId, (scanId, previousId) => {
  if (scanId && scanId !== previousId) {
    scanGeneration += 1
    stopAccepted.value = false
    scanReport.value = undefined
  }
}, { flush: 'sync' })
watch(() => devices.scanning, (running, wasRunning) => {
  if (wasRunning && !running && !startingScan.value && !cancellingScan.value && !scanError.value && scanReport.value !== 'cached')
    scanReport.value = stopAccepted.value ? 'stopRequested' : 'finished'
})

const forgetTarget = ref<DeviceCard>()
const forgetError = ref<IpcError>()
const forgetting = ref(false)
const notice = ref('')
const keepButton = ref<InstanceType<typeof AButton>>()
const pairTrigger = ref<InstanceType<typeof AButton>>()
const pairedHeading = ref<HTMLElement>()
let forgetTrigger: HTMLElement | null = null
async function askForget(card: DeviceCard): Promise<void> {
  if (forgetting.value || !devices.get(card.deviceId)?.paired)
    return
  forgetTrigger = document.activeElement instanceof HTMLElement ? document.activeElement : null
  forgetTarget.value = card
  forgetError.value = undefined
  await nextTick()
  if (!disposed)
    keepButton.value?.$el.focus()
}
async function closeForget(): Promise<void> {
  if (forgetting.value)
    return
  forgetTarget.value = undefined
  forgetError.value = undefined
  await nextTick()
  if (!disposed) {
    if (forgetTrigger?.isConnected)
      forgetTrigger.focus()
    else
      pairTrigger.value?.$el.focus()
  }
}
async function forget(): Promise<void> {
  const target = forgetTarget.value
  if (forgetting.value || !target || !devices.get(target.deviceId)?.paired)
    return
  forgetting.value = true
  forgetError.value = undefined
  try {
    const answer = await kernel.call<null>('devices_forget', { deviceId: target.deviceId })
    if (disposed)
      return
    if (!answer.ok) {
      forgetError.value = answer.error
      return
    }
    // The command has removed the relationship even when a subsequent read fails.
    devices.remove(target.deviceId)
    notice.value = messages.devices.forgotten(target.displayName)
    forgetTarget.value = undefined
    await refresh()
    await nextTick()
    if (!disposed)
      (pairedHeading.value ?? pairTrigger.value?.$el)?.focus()
  }
  finally {
    forgetting.value = false
  }
}

onScopeDispose(() => {
  disposed = true
  readRevision += 1
})
onMounted(() => {
  void refresh()
})
</script>
