<template>
  <AModal v-if="pairing.visible" :open="true" :title="messages.devices.pairingWith(pairing.peerName)" :get-container="false" :footer="null" :closable="!pairing.busy" :mask-closable="false" @cancel="pairing.close">
    <div class="stack gap-4" aria-live="polite">
      <template v-if="pairing.snapshot.phase === 'asked' && pairing.snapshot.ask">
        <p class="m-0 text-sm">
          {{ messages.devices.pairingAsk(pairing.snapshot.ask.displayName) }}
        </p>
        <output class="m-0 text-center text-3xl font-mono" :aria-label="messages.devices.pairingCode">{{ pairing.snapshot.ask.code }}</output>
        <div class="row flex-wrap justify-end gap-2 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8">
          <AButton danger :disabled="pairing.busy" @click="pairing.respond(false, true)">
            {{ messages.devices.pairingBlock }}
          </AButton>
          <AButton :disabled="pairing.busy" @click="pairing.respond(false)">
            {{ messages.devices.pairingRefuse }}
          </AButton>
          <AButton type="primary" :disabled="pairing.busy" @click="pairing.respond(true)">
            {{ messages.devices.pairingAccept }}
          </AButton>
        </div>
      </template>
      <template v-else-if="pairing.snapshot.phase === 'waiting' && pairing.snapshot.offer">
        <p class="m-0 text-sm">
          {{ messages.devices.pairingWaiting(pairing.snapshot.offer.displayName) }}
        </p>
        <output class="m-0 text-center text-3xl font-mono" :aria-label="messages.devices.pairingCode">{{ pairing.snapshot.offer.code }}</output>
        <p class="m-0 text-xs text-ink-dim">
          {{ messages.devices.pairingWaitingHelp }}
        </p>
      </template>
      <p v-else-if="outcomeText" class="m-0 text-sm">
        {{ outcomeText }}
      </p>
      <p v-if="pairing.unreachable" class="m-0 text-sm">
        {{ messages.devices.pairingUnreachable }}
      </p>
      <ErrorNotice v-if="pairing.error" :code="pairing.error.code" :params="pairing.error.params" :actions="pairing.error.actions?.filter(action => action.kind !== 'retry')" />
      <div v-if="pairing.snapshot.phase !== 'asked'" class="row justify-end gap-2 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8">
        <AButton v-if="pairing.snapshot.phase === 'waiting'" :loading="pairing.busy" :disabled="pairing.busy" @click="pairing.cancel">
          {{ messages.devices.pairingWithdraw }}
        </AButton>
        <AButton v-else type="primary" :disabled="pairing.busy" @click="pairing.close">
          {{ messages.devices.pairingClose }}
        </AButton>
      </div>
    </div>
  </AModal>
</template>

<script setup lang="ts">
import { Button as AButton, Modal as AModal } from 'ant-design-vue'
import { computed } from 'vue'
import { messages } from '../messages'
import { usePairingStore } from '../stores/pairing'
import ErrorNotice from './ErrorNotice.vue'

const pairing = usePairingStore()
const outcomeText = computed(() => {
  switch (pairing.snapshot.outcome) {
    case 'paired': return messages.devices.pairingPaired(pairing.peerName)
    case 'refused': return messages.devices.pairingRefused(pairing.peerName)
    case 'cancelled': return messages.devices.pairingCancelled
    case 'timedOut': return messages.devices.pairingExpired
    default: return undefined
  }
})
</script>
