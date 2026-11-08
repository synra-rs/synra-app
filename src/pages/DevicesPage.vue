<template>
  <!--
    A workspace page, but a narrow one: paired and nearby devices are two short lists whose rows carry a name, a
    platform and a couple of actions. At the shell's full 1200px those actions drifted ~700px from the name they belong
    to, which is the "controls pulled apart across a wide window" [03 §3](../../design-docs/ui-design/03-shell-and-navigation.md)
    warns against. The workspace width is therefore capped at 896px — wider than the reading measure, because the rows
    hold a name and an action set on one line, and narrower than a full workspace, because nothing here fills it.
  -->
  <section class="mx-auto max-w-4xl min-w-0 pb-4" data-page-width="narrow-workspace">
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
        <p v-if="devices.cards.length" class="m-0 text-sm text-ink-dim">
          {{ messages.devices.knownOnly }}
        </p>
        <div>
          <AButton :loading="refreshing" :disabled="refreshing" @click="refresh">
            {{ messages.devices.refresh }}
          </AButton>
        </div>
      </div>
      <p v-if="notice" class="m-0 text-base" role="status">
        {{ notice }}
      </p>

      <div v-if="scanning || scanReport || scanError" class="stack gap-2" aria-live="polite">
        <div v-if="scanError" class="stack gap-2">
          <ErrorNotice :code="scanError.code" :params="scanError.params" :actions="scanError.actions?.filter(action => action.kind !== 'retry')" />
          <p v-if="devices.cards.length" class="m-0 text-sm text-ink-dim">
            {{ messages.devices.scanFailedKnown }}
          </p>
          <div v-if="scanError.retryable">
            <AButton :disabled="cancellingScan || (!failedCancelId && scanning)" @click="retryScan">
              {{ messages.actions.retry }}
            </AButton>
          </div>
        </div>
        <p v-else class="m-0 row flex-wrap gap-2 text-base text-ink-dim">
          <span v-if="scanning" class="i-carbon-circle-dash animate-spin" aria-hidden="true" />
          <span>{{ scanSummary }}</span>
          <span v-if="scanState">{{ messages.devices.scanProgress }} {{ scanState.found }}</span>
        </p>
        <details v-if="scanState" class="text-sm text-ink-dim">
          <summary class="synra-choice min-h-8 row cursor-pointer">
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

      <p v-if="refreshing && !ready && !devices.cards.length" class="m-0 text-base text-ink-dim" role="status">
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

      <section v-if="devices.paired.length" :aria-label="messages.devices.paired" class="min-w-0 stack gap-0">
        <h2 ref="pairedHeading" class="m-0 text-base font-medium" tabindex="-1">
          {{ messages.devices.paired }}
        </h2>
        <DeviceRow v-for="card in devices.paired" :key="card.deviceId" :card="card" @forget="askForget" />
      </section>
      <section v-if="nearby.length" :aria-label="messages.devices.nearby" class="min-w-0 stack gap-0">
        <h2 class="m-0 text-base font-medium">
          {{ messages.devices.nearby }}
        </h2>
        <DeviceRow v-for="card in nearby" :key="card.deviceId" :card="card" @pair="startPair" @forget="askForget" />
      </section>
    </div>

    <AModal v-if="forgetTarget" :open="forgetVisible" :title="messages.devices.forgetTitle(forgetTarget.displayName)" :footer="null" :closable="false" :mask-closable="!forgetting" :keyboard="!forgetting" @cancel="closeForget">
      <div class="stack gap-4">
        <p class="m-0 text-base">
          {{ messages.devices.forgetHelp }}
        </p>
        <p class="m-0 break-all text-sm text-ink-dim font-mono">
          {{ forgetTarget.deviceId }}
        </p>
        <ErrorNotice v-if="forgetError" :code="forgetError.code" :params="forgetError.params" :actions="forgetError.actions?.filter(action => action.kind !== 'retry')" />
        <div class="row justify-end gap-2">
          <AButton ref="keepButton" :disabled="forgetting" @click="closeForget">
            {{ messages.devices.forgetCancel }}
          </AButton>
          <AButton type="primary" danger :loading="forgetting" :disabled="forgetting || !(devices.get(forgetTarget.deviceId)?.paired || devices.get(forgetTarget.deviceId)?.blocked)" @click="forget">
            {{ messages.devices.forget }}
          </AButton>
        </div>
      </div>
    </AModal>
  </section>
</template>

<script setup lang="ts">
import type { DeviceCard, IpcError } from '@synra/protocol'
import { Button as AButton, Empty as AEmpty, Modal as AModal } from 'ant-design-vue'
import { computed, nextTick, onScopeDispose, ref } from 'vue'
import { kernel } from '../app/kernel'
import ErrorNotice from '../components/ErrorNotice.vue'
import PageHeader from '../components/PageHeader.vue'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'
import { useDialogVisibility } from '../stores/dialogs'
import { usePairingStore } from '../stores/pairing'
import DeviceRow from './devices/DeviceRow.vue'
import { useDeviceDiscovery } from './devices/use-device-discovery'

const devices = useDevicesStore()
const pairing = usePairingStore()
const startPair = pairing.start
const nearby = computed(() => devices.cards.filter(card => !card.paired))
let disposed = false
const { snapshotError, refreshing, ready, refresh, startingScan, cancellingScan, scanState, scanning, scanReport, scanError, failedCancelId, scanSummary, scan, cancelScan, retryScan } = useDeviceDiscovery()
const forgetTarget = ref<DeviceCard>()
const forgetVisible = useDialogVisibility(computed(() => !!forgetTarget.value), 60)
const forgetError = ref<IpcError>()
const forgetting = ref(false)
const notice = ref('')
const keepButton = ref<InstanceType<typeof AButton>>()
const pairedHeading = ref<HTMLElement>()
let forgetTrigger: HTMLElement | null = null
async function askForget(card: DeviceCard): Promise<void> {
  if (forgetting.value || !(devices.get(card.deviceId)?.paired || devices.get(card.deviceId)?.blocked))
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
  if (forgetting.value || !target || !(devices.get(target.deviceId)?.paired || devices.get(target.deviceId)?.blocked))
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
})
</script>
