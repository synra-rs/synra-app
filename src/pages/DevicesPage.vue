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
      </PageHeader>

      <div v-if="snapshotError" class="stack gap-2">
        <ErrorNotice :code="snapshotError.code" :params="snapshotError.params" :actions="snapshotError.actions?.filter(action => action.kind !== 'retry')" />
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

      <div v-if="scanning || scanReport || scanError" class="stack gap-2" aria-live="polite">
        <div v-if="scanError" class="stack gap-2">
          <ErrorNotice :code="scanError.code" :params="scanError.params" :actions="scanError.actions?.filter(action => action.kind !== 'retry')" />
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

      <section v-if="devices.paired.length" :aria-label="messages.devices.paired" class="min-w-0 stack gap-3 border border-line rounded-surface bg-glass p-4">
        <h2 ref="pairedHeading" class="m-0 text-sm font-medium" tabindex="-1">
          {{ messages.devices.paired }}
        </h2>
        <DeviceRow v-for="card in devices.paired" :key="card.deviceId" :card="card" @forget="askForget" />
      </section>
      <section v-if="nearby.length" :aria-label="messages.devices.nearby" class="min-w-0 stack gap-3 border border-line rounded-surface bg-glass p-4">
        <h2 class="m-0 text-sm font-medium">
          {{ messages.devices.nearby }}
        </h2>
        <DeviceRow v-for="card in nearby" :key="card.deviceId" :card="card" @pair="startPair" />
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
        <ErrorNotice v-if="forgetError" :code="forgetError.code" :params="forgetError.params" :actions="forgetError.actions?.filter(action => action.kind !== 'retry')" />
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
    <AModal v-if="pairing && (pairing.phase !== 'idle' || pairingUnreachable)" :open="true" :title="messages.devices.pairingWith(pairing.offer?.displayName ?? pairing.ask?.displayName ?? pairingTarget?.displayName ?? '')" :get-container="false" :footer="null" :closable="true" :mask-closable="false" @cancel="closePairing">
      <div class="stack gap-4">
        <template v-if="pairing.phase === 'asked' && pairing.ask">
          <p class="m-0 text-sm">
            {{ messages.devices.pairingAsk(pairing.ask.displayName) }}
          </p>
          <output class="m-0 text-center text-3xl font-mono tracking-[0.3em]">{{ pairing.ask.code }}</output>
          <div class="row justify-end gap-2 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8">
            <AButton :loading="pairingBusy" @click="respondPair(false)">
              {{ messages.devices.pairingRefuse }}
            </AButton>
            <AButton type="primary" :loading="pairingBusy" @click="respondPair(true)">
              {{ messages.devices.pairingAccept }}
            </AButton>
          </div>
        </template>        <template v-else-if="pairing.phase === 'waiting' && pairing.offer">
          <p class="m-0 text-sm">
            {{ messages.devices.pairingWaiting(pairing.offer.displayName) }}
          </p>
          <output class="m-0 text-center text-3xl font-mono tracking-[0.3em]">{{ pairing.offer.code }}</output>
          <p class="m-0 text-xs text-ink-dim">
            {{ messages.devices.pairingWaitingHelp }}
          </p>
        </template>
        <p v-else-if="pairingOutcomeText" class="m-0 text-sm">
          {{ pairingOutcomeText }}
        </p>
        <p v-if="pairingUnreachable" class="m-0 text-sm">
          {{ messages.devices.pairingUnreachable }}
        </p>
        <ErrorNotice v-if="pairingError" :code="pairingError.code" :params="pairingError.params" :actions="pairingError.actions?.filter(action => action.kind !== 'retry')" />
        <div class="row justify-end gap-2 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8">
          <AButton v-if="pairing.phase === 'waiting'" :loading="pairingBusy" @click="withdrawPair">
            {{ messages.devices.pairingWithdraw }}
          </AButton>
          <AButton type="primary" @click="closePairing">
            {{ messages.devices.pairingClose }}
          </AButton>
        </div>
      </div>
    </AModal>
  </section>
</template>

<script setup lang="ts">
import type { DeviceCard, DevicesSnapshot, IpcError, PairingSnapshot, ScanOptions } from '@synra/protocol'
import { Button as AButton, Empty as AEmpty, Modal as AModal } from 'ant-design-vue'
import { computed, nextTick, onScopeDispose, ref, watch } from 'vue'
import ErrorNotice from '../components/ErrorNotice.vue'
import PageHeader from '../components/PageHeader.vue'
import { kernel } from '../kernel'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'
import DeviceRow from './devices/DeviceRow.vue'

const devices = useDevicesStore()
/**
 * Pairing: the six digits this device shows, and the answer the other person gives.
 *
 * ★ The snapshot arrives as an event rather than from a poll. A poll would leave the panel up to a
 * second behind the desk, and the desk is the one thing both people are looking at.
 */
const pairing = ref<PairingSnapshot>()
const pairingTarget = ref<DeviceCard>()
const pairingError = ref<IpcError>()
const pairingBusy = ref(false)
const pairingUnreachable = ref(false)
let stopPairingEvents: (() => void) | undefined
void kernel.subscribe('kernel-event', (payload) => {
  const event = payload as { type?: string, payload?: unknown }
  if (event.type === 'pairingUpdated' && event.payload) {
    const next = event.payload as PairingSnapshot
    // ★ 拨号失败时守护进程把桌子清空，事件是 idle：界面若不解释一句，弹窗会静悄悄消失。
    if (pairing.value?.phase === 'waiting' && next.phase === 'idle')
      pairingUnreachable.value = true
    pairing.value = next
  }
}).then((off) => {
  stopPairingEvents = off
})
onScopeDispose(() => {
  stopPairingEvents?.()
})

async function startPair(card: DeviceCard): Promise<void> {
  pairingTarget.value = card
  pairingError.value = undefined
  pairingUnreachable.value = false
  pairingBusy.value = true
  const answer = await kernel.call<PairingSnapshot>('devices_pair', { deviceId: card.deviceId })
  pairingBusy.value = false
  if (answer.ok)
    pairing.value = answer.value
  else
    pairingError.value = answer.error
}

async function withdrawPair(): Promise<void> {
  pairingBusy.value = true
  const answer = await kernel.call<PairingSnapshot>('devices_pairing_cancel')
  pairingBusy.value = false
  if (answer.ok)
    pairing.value = answer.value
  else
    pairingError.value = answer.error
}


async function respondPair(accepted: boolean): Promise<void> {
  pairingBusy.value = true
  const answer = await kernel.call<PairingSnapshot>('devices_pairing_respond', { accepted })
  pairingBusy.value = false
  if (answer.ok)
    pairing.value = answer.value
  else
    pairingError.value = answer.error
}

/**
 * Closing a prompt is an answer, not silence.
 *
 * ★ 一条挂着的请求在对面看来是「还在等」。X 掉窗口而不答复，会让那台设备一直摆到两分钟到期，
 * 也会让本机的配对桌一直被占着。所以：我在等对方 → 关闭等于撤回；对方在等我 → 关闭等于拒绝。
 */
async function closePairing(): Promise<void> {
  const phase = pairing.value?.phase
  if (phase === 'waiting')
    await kernel.call('devices_pairing_cancel')
  else if (phase === 'asked')
    await kernel.call('devices_pairing_respond', { accepted: false })
  // ★ 配对成功后两边的列表都该换一边：关掉提示时重新读一次，''已配对'' 才不会留在''附近''里。
  if (pairing.value?.outcome === 'paired')
    void refresh()
  pairing.value = undefined
  pairingTarget.value = undefined
  pairingError.value = undefined
  pairingUnreachable.value = false
}

/**
 * Incoming requests, read once when the page opens.
 *
 * ★ Later changes arrive as events; this single read covers the case where a request was already on the
 * desk before this page mounted.
 */
void kernel.call<PairingSnapshot>('devices_pairing_snapshot').then((answer) => {
  if (answer.ok && answer.value.phase !== 'idle' && !pairing.value)
    pairing.value = answer.value
})
const pairingOutcomeText = computed(() => {
  const name = pairingTarget.value?.displayName ?? ''
  switch (pairing.value?.outcome) {
    case 'paired': return messages.devices.pairingPaired(name)
    case 'refused': return messages.devices.pairingRefused(name)
    case 'cancelled': return messages.devices.pairingCancelled
    case 'timedOut': return messages.devices.pairingExpired
    default: return undefined
  }
})
const nearby = computed(() => devices.cards.filter(card => !card.paired))
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
      pairedHeading.value?.focus()
  }
  finally {
    forgetting.value = false
  }
}

onScopeDispose(() => {
  disposed = true
  readRevision += 1
})
</script>


