<style scoped>
/*
 * ★ `09 §3` sets 44px as the floor for a thumb — the same number `min-h-11` carries. A `.ant-btn` rendered by the
 * library is not a utility-classed element, so it needs the rule written out.
 *
 * ★ 639 rather than 767: `03 §3`'s narrow layout is `<640`, and the utility route drops back to 32px at `md` (768).
 *   Ending here keeps every surface on one screen agreeing about which layout it is in —
 *   `check-shapes.ts`'s `touch-breakpoints` holds the number.
 *
 * The rule lives on the body rather than the dialog chrome because the body is what draws the buttons: the shell it
 * sits in (`Modal` or `Drawer`) owns none of them, and a rule up there would be scoping a floor onto nothing.
 */
@media (max-width: 639px), (pointer: coarse) {
  .pairing-panel :deep(.ant-btn) {
    min-height: 44px;
  }
}
</style>

<template>
  <div class="stack gap-4" data-testid="pairing-body">
    <!--
      ★ The requests other devices are waiting on this person to decide about.

        A device asks; the kernel files the request on the desk and parks that connection until someone answers. Nothing
        announced it to this page — a request arrives on a socket thread — so the list is read on a timer.

        Every request shows the same six digits the other device shows. That is the only thing that establishes both
        screens are looking at one request, and it is why it is stated as a comparison rather than as an identifier: the
        digits prove agreement and nothing about who the device is.
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
        <!--
          ★ The code, large and monospaced.

          A person reads this off one screen and compares it with the other. It is the one thing on this panel that has to
          be legible at a glance across a desk, so it gets its own size rather than sharing the body text.
        -->
        <p class="m-0 text-center text-2xl font-mono" data-testid="pairing-code">
          {{ prompt.verificationCode }}
        </p>
        <p class="m-0 text-center text-xs text-ink-dim">
          {{ messages.devices.pairingRequestCompare }}
        </p>
        <p class="m-0 break-all font-mono text-xs text-ink-dim">
          {{ prompt.deviceId }}
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
      ★ Nothing running and a device selected: the outward half is not wired yet.

        Said plainly rather than shown as a button that cannot work. What it needs is a client for `synra/pairing/1` —
        the protocol has the server side, so this device can answer a request, but nothing on it can open one.
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

    <!-- Nothing running and nothing selected: the list behind this dialog is where a person picks a device. -->
    <HelpText v-else>
      {{ messages.devices.pairingChooseTarget }}
    </HelpText>
  </div>
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

withDefaults(defineProps<{ selected?: DeviceCard | undefined }>(), { selected: undefined })
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

async function restart(): Promise<void> {
  dismissedRequest.value = pairing.snapshot?.requestId
  await nextTick()
  if (!disposed)
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
  // One read on mount, not two: this body is mounted once per opening, and the polling timer covers everything after.
  void pairing.refreshPending()
  const poll = setInterval(() => {
    void pairing.refreshPending()
  }, REQUEST_POLL_MS)
  onScopeDispose(() => clearInterval(poll))
})

onScopeDispose(() => {
  disposed = true
})
</script>
