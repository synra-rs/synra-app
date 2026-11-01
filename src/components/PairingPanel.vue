<style scoped>
/*
 * ★ `09 §3` sets 44px as the floor for a thumb — the same number `min-h-11` carries (11 × 4px spacing) and the one
 *   `App.vue`'s bottom bar restates. Three places, one number, no constant shared between them: the utility has it
 *   baked in, and this rule exists because a `.ant-btn` rendered by the library is not a utility-classed element.
 *
 * ★ 639 rather than 767: `03 §3`'s narrow layout is `<640`, and the utility route drops back to 32px at `md` (768).
 *   Ending here keeps every surface on one screen agreeing about which layout it is in —
 *   `check-shapes.ts`'s `touch-breakpoints` holds the number.
 */
@media (max-width: 639px), (pointer: coarse) {
  .pairing-panel :deep(.ant-btn) {
    min-height: 44px;
  }
}
</style>

<template>
  <section class="pairing-panel stack gap-4 border border-line rounded-surface border-solid p-4" aria-labelledby="pairing-title">
    <h2 id="pairing-title" class="m-0 text-base font-medium">
      {{ messages.devices.pairingTitle }}
    </h2>

    <!--
      ★ The requests other devices are waiting on this person to decide about.

        A device asks; the kernel files the request on the desk and parks that connection until someone answers. Nothing
        announced it to this page — a request arrives on a socket thread — so the list is read on a timer.

        Not a modal: several can be waiting, and one blocking the rest of the page would put the second out of reach.
        Each carries its own answer, and a row leaves only after the kernel says it was answered.
    -->
    <div v-if="pairing.pending.length > 0" class="stack gap-3" data-testid="pairing-requests">
      <h3 class="m-0 text-sm font-medium">
        {{ messages.devices.pairingRequests }}
      </h3>
      <ErrorNotice v-if="pairing.pendingError" :code="pairing.pendingError.code" :params="pairing.pendingError.params" />
      <div
        v-for="prompt in pairing.pending"
        :key="prompt.requestId"
        class="stack gap-2 border border-line rounded-surface border-solid p-3"
      >
        <p class="m-0 break-words text-sm">
          {{ messages.devices.pairingRequestWants(prompt.displayName) }}
        </p>
        <!-- The whole id: two devices can share a name, and the name is only what the other one claimed. -->
        <p class="m-0 break-all font-mono text-xs text-ink-dim">
          {{ prompt.deviceId }}
        </p>
        <!--
          The code is the check: the other device shows the same six characters and the person compares them. Worded as
          a comparison rather than an identifier, because it proves both screens hold the same request and no more.
        -->
        <p class="m-0 text-sm">
          {{ messages.devices.pairingRequestCompare(prompt.verificationCode) }}
        </p>
        <p v-if="prompt.address" class="m-0 text-xs text-ink-dim">
          {{ messages.devices.pairingRequestAddress(prompt.address) }}
        </p>
        <p v-if="prompt.expiresInMs > 0" class="m-0 text-xs text-ink-dim">
          {{ messages.devices.pairingRequestExpires(Math.ceil(prompt.expiresInMs / 1000)) }}
        </p>
        <div class="row flex-wrap gap-2">
          <AButton
            type="primary"
            :loading="pairing.answering === prompt.requestId"
            :disabled="pairing.answering !== null && pairing.answering !== prompt.requestId"
            @click="pairing.answer(prompt.requestId, true)"
          >
            {{ messages.devices.pairingRequestAccept }}
          </AButton>
          <AButton
            :disabled="pairing.answering !== null"
            @click="pairing.answer(prompt.requestId, false)"
          >
            {{ messages.devices.pairingRequestDecline }}
          </AButton>
        </div>
      </div>
    </div>

    <ErrorNotice
      v-if="pairing.commandError"
      :code="pairing.commandError.code"
      :params="pairing.commandError.params"
      :actions="pairing.commandError.actions?.filter(action => action.kind !== 'retry')"
    />
    <div v-if="pairing.statusError" class="stack gap-2">
      <ErrorNotice :code="pairing.statusError.code" :params="pairing.statusError.params" />
      <AButton :loading="pairing.refreshing" :disabled="pairing.refreshing || pairing.busy" @click="pairing.refresh()">
        {{ messages.devices.pairingRefresh }}
      </AButton>
    </div>

    <!-- A task already running: what it is, how far it has got, and how to stop it. -->
    <div v-if="currentTask" class="stack gap-3" role="status">
      <p class="m-0 break-words text-sm">
        {{ messages.devices.pairingPeer(taskName) }}
      </p>
      <p ref="taskStatus" class="m-0 text-sm font-medium" tabindex="-1">
        {{ messages.devices.pairingPhases[currentTask.phase] }}
      </p>
      <p v-if="pairing.active" class="m-0 text-xs text-ink-dim">
        {{ messages.devices.pairingExpires }} {{ formatDateTime(currentTask.expiresAtMs) }}
      </p>
      <!-- Cancelling belongs to the side that asked. The other side accepts or declines, and has no "cancel". -->
      <template v-if="pairing.active">
        <HelpText>{{ messages.devices.pairingCloseHelp }}</HelpText>
        <div>
          <AButton :loading="pairing.busy" :disabled="pairing.busy || currentTask.phase === 'cancelling'" @click="pairing.cancel()">
            {{ messages.devices.pairingCancel }}
          </AButton>
        </div>
      </template>
      <div v-else>
        <AButton :disabled="pairing.busy || pairing.refreshing || !!pairing.statusError" @click="restart">
          {{ messages.devices.pairingRestart }}
        </AButton>
      </div>
    </div>

    <!--
      ★ A device is selected and nothing is running: the outward half is not wired yet.

        Said plainly rather than shown as a button that cannot work. A control that looks pressable and then refuses is
        what this panel spent its whole life doing; a sentence at least tells the person where they stand.

        What it needs is a client for `synra/pairing/1` — the protocol has the server side, so a device can answer a
        request, but nothing can open one.
    -->
    <div v-else-if="selected" class="stack gap-3" data-testid="pairing-not-wired">
      <p class="m-0 break-words text-sm">
        {{ messages.devices.pairingRequestPeer(selected.displayName) }}
      </p>
      <p class="m-0 break-all font-mono text-xs text-ink-dim">
        {{ selected.deviceId }}
      </p>
      <HelpText>{{ messages.devices.pairingRequestUnavailable }}</HelpText>
    </div>

    <!-- Nothing running and nothing selected: the list above is where a person picks a device. -->
    <HelpText v-else>
      {{ messages.devices.pairingChooseTarget }}
    </HelpText>
  </section>
</template>

<script setup lang="ts">
import type { DeviceCard } from '@synra/protocol'
import { Button as AButton } from 'ant-design-vue'
import { computed, nextTick, onMounted, onScopeDispose, ref } from 'vue'
import { formatDateTime } from '../locale-format'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'
import { usePairingStore } from '../stores/pairing'
import ErrorNotice from './ErrorNotice.vue'
import HelpText from './HelpText.vue'

const props = withDefaults(defineProps<{ selected?: DeviceCard | undefined, visible?: boolean }>(), { visible: true })
const devices = useDevicesStore()
const pairing = usePairingStore()
const taskStatus = ref<HTMLElement>()
let disposed = false

/** A task worth showing: running, or finished and not yet dismissed. */
const dismissedRequest = ref<string>()
const currentTask = computed(() => pairing.snapshot && (pairing.active || pairing.snapshot.requestId !== dismissedRequest.value)
  ? pairing.snapshot
  : null)

const taskName = computed(() => {
  const id = currentTask.value?.peerId ?? ''
  const name = devices.get(id)?.displayName
  return name ? `${name} (${id})` : id
})

const locked = computed(() => pairing.busy || pairing.active || pairing.refreshing || !!pairing.statusError)

async function restart(): Promise<void> {
  if (locked.value)
    return
  dismissedRequest.value = pairing.snapshot?.requestId
  await nextTick()
  if (!disposed && props.visible !== false)
    taskStatus.value?.focus()
}

/**
 * How often the waiting-request list is read.
 *
 * Three seconds: a pairing window is tens of seconds, so noticing a second late costs a second of a person's wait.
 * Every frame would spend a round trip per frame to shorten that, on a list that is usually empty.
 */
const REQUEST_POLL_MS = 3000

onMounted(() => {
  void pairing.refresh()
  const poll = setInterval(() => {
    void pairing.refreshPending()
  }, REQUEST_POLL_MS)
  onScopeDispose(() => clearInterval(poll))
  void pairing.refreshPending()
})

onScopeDispose(() => {
  disposed = true
})
</script>
