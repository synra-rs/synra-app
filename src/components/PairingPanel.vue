<template>
  <section class="stack gap-3 border border-line rounded border-solid p-3" aria-labelledby="pairing-title">
    <h2 id="pairing-title" class="m-0 text-base font-medium">
      {{ messages.devices.pairingTitle }}
    </h2>
    <HelpText>
      {{ messages.devices.pairingInstructions }}
    </HelpText>
    <ErrorNotice v-if="pairing.error" :code="pairing.error.code" :message="pairing.error.message" :actions="pairing.error.actions" @retry="pairing.refresh()" />
    <AButton :loading="pairing.busy" :disabled="pairing.busy" @click="pairing.exportInvitation()">
      {{ messages.devices.pairingExport }}
    </AButton>
    <label v-if="pairing.invitation" class="stack gap-1 text-sm">
      {{ messages.devices.pairingInvitation }}
      <ATextarea :value="pairing.invitation" readonly :rows="3" />
    </label>
    <template v-if="selected">
      <p class="m-0 text-sm">
        {{ messages.devices.pairingSelected(selected.displayName, selected.deviceId) }}
      </p>
      <label class="stack gap-1 text-sm">
        {{ messages.devices.pairingImport }}
        <ATextarea v-model:value="imported" :maxlength="2200" :rows="3" :disabled="pairing.busy || pairing.active" />
      </label>
      <div class="row flex-wrap gap-2">
        <AButton :loading="pairing.busy" :disabled="cannotStart" @click="pairing.listen(selected.deviceId, imported)">
          {{ messages.devices.pairingListen }}
        </AButton>
        <AButton :loading="pairing.busy" :disabled="cannotStart" @click="pairing.connect(selected.deviceId, imported)">
          {{ messages.devices.pairingConnect }}
        </AButton>
      </div>
    </template>
    <div v-if="pairing.snapshot" class="stack gap-2" role="status">
      <p class="m-0 text-sm">
        {{ messages.devices.pairingPeer(pairing.snapshot.peerId) }}
      </p>
      <p class="m-0 text-sm">
        {{ messages.devices.pairingPhases[pairing.snapshot.phase] }}
      </p>
      <p v-if="pairing.active" class="m-0 text-sm">
        {{ messages.devices.pairingExpires }}
        <time :datetime="new Date(pairing.snapshot.expiresAtMs).toISOString()">{{ new Date(pairing.snapshot.expiresAtMs).toLocaleTimeString() }}</time>
      </p>
      <label v-if="pairing.snapshot.reply && pairing.active" class="stack gap-1 text-sm">
        {{ messages.devices.pairingReply }}
        <ATextarea :value="pairing.snapshot.reply" readonly :rows="3" />
      </label>
      <ErrorNotice v-if="pairing.snapshot.errorCode" :code="pairing.snapshot.errorCode" :message="`error.${pairing.snapshot.errorCode}`" />
      <AButton v-if="pairing.active" :disabled="pairing.busy || pairing.snapshot.phase === 'cancelling'" @click="pairing.cancel()">
        {{ messages.devices.pairingCancel }}
      </AButton>
    </div>
  </section>
</template>

<script setup lang="ts">
import type { DeviceCard } from '@synra/protocol'
import { Button as AButton, Textarea as ATextarea } from 'ant-design-vue'
import { computed, onMounted, ref, watch } from 'vue'
import { messages } from '../messages'

import { usePairingStore } from '../stores/pairing'
import ErrorNotice from './ErrorNotice.vue'
import HelpText from './HelpText.vue'

const props = defineProps<{ selected?: DeviceCard | undefined }>()
const pairing = usePairingStore()
const imported = ref('')
const cannotStart = computed(() => pairing.busy || pairing.active || props.selected?.paired === true || imported.value.trim().length === 0)
watch(() => props.selected?.deviceId, () => {
  imported.value = ''
})
onMounted(() => {
  void pairing.refresh()
})
</script>
