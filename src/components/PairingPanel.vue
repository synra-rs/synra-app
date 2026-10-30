<style scoped>
@media (max-width: 767px), (pointer: coarse) {
  .pairing-panel :deep(.ant-btn) {
    min-height: 44px;
  }

  .pairing-panel :deep(.ant-select-single) {
    height: 44px;
  }
}
</style>

<template>
  <section class="pairing-panel stack gap-4 border border-line rounded border-solid p-4" aria-labelledby="pairing-title">
    <h2 id="pairing-title" class="m-0 text-base font-medium">
      {{ messages.devices.pairingTitle }}
    </h2>
    <ErrorNotice
      v-if="pairing.commandError"
      :code="pairing.commandError.code"
      :message="pairing.commandError.message"
      :actions="pairing.commandError.actions?.filter(action => action.kind !== 'retry')"
    />
    <div v-if="pairing.statusError" class="stack gap-2">
      <ErrorNotice :code="pairing.statusError.code" :message="pairing.statusError.message" />
      <AButton :loading="pairing.refreshing" :disabled="pairing.refreshing || pairing.busy" @click="pairing.refresh()">
        {{ messages.devices.pairingRefresh }}
      </AButton>
    </div>

    <div v-if="currentTask" class="stack gap-3" role="status">
      <p class="m-0 break-words text-sm">
        {{ messages.devices.pairingPeer(taskName) }}
      </p>
      <p ref="taskStatus" class="m-0 text-sm font-medium" tabindex="-1">
        {{ messages.devices.pairingPhases[currentTask.phase] }}
      </p>
      <p v-if="pairing.active" class="m-0 text-xs text-ink-dim">
        {{ messages.devices.pairingExpires }}
        <time :datetime="new Date(currentTask.expiresAtMs).toISOString()">{{ formatDateTime(currentTask.expiresAtMs) }}</time>
      </p>
      <label v-if="currentTask.reply && pairing.active" class="stack gap-2 text-sm">
        {{ messages.devices.pairingReply }}
        <ATextarea :value="currentTask.reply" readonly :rows="3" />
      </label>
      <div v-if="currentTask.reply && pairing.active">
        <AButton :loading="copyState === 'pending'" :disabled="copyState === 'pending'" @click="copy(currentTask.reply)">
          {{ copyState === 'copied' ? messages.devices.pairingCopied : messages.devices.pairingCopy }}
        </AButton>
      </div>
      <ErrorNotice v-if="currentTask.errorCode" :code="currentTask.errorCode" :message="`error.${currentTask.errorCode}`" />
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

    <template v-else>
      <label class="max-w-lg stack gap-2 text-sm">
        {{ messages.devices.pairingRole }}
        <ASelect ref="stepControl" v-model:value="step" :aria-label="messages.devices.pairingRole" :options="steps" :disabled="locked" />
      </label>

      <template v-if="step === 'create'">
        <HelpText>{{ messages.devices.pairingCreateHelp }}</HelpText>
        <div v-if="!pairing.invitation">
          <AButton type="primary" :loading="pairing.busy" :disabled="locked" @click="generate">
            {{ messages.devices.pairingExport }}
          </AButton>
        </div>
        <template v-else>
          <label class="stack gap-2 text-sm">
            {{ messages.devices.pairingInvitation }}
            <ATextarea ref="invitationControl" :value="pairing.invitation" readonly :rows="3" />
          </label>
          <div class="row flex-wrap gap-2">
            <AButton :loading="copyState === 'pending'" :disabled="copyState === 'pending'" @click="copy(pairing.invitation)">
              {{ copyState === 'copied' ? messages.devices.pairingCopied : messages.devices.pairingCopy }}
            </AButton>
            <AButton type="primary" :disabled="locked" @click="goToReply">
              {{ messages.devices.pairingReplyStep }}
            </AButton>
          </div>
        </template>
      </template>

      <template v-else>
        <label class="max-w-lg stack gap-2 text-sm">
          {{ messages.devices.pairingTarget }}
          <ASelect
            ref="targetControl"
            :value="selected?.deviceId"
            :options="targets"
            :aria-label="messages.devices.pairingTarget"
            :placeholder="messages.devices.pairingChooseTarget"
            :disabled="locked"
            @change="selectTarget"
          >
            <template #option="{ label, value }">
              <span class="block">{{ label }}</span>
              <span class="block whitespace-normal break-all text-xs text-ink-dim">{{ value }}</span>
            </template>
          </ASelect>
        </label>
        <HelpText v-if="targets.length === 0">
          {{ messages.devices.pairingNoTarget }}
        </HelpText>
        <template v-if="selected">
          <p class="m-0 break-words text-xs text-ink-dim">
            {{ messages.devices.pairingSelected(selected.displayName, selected.deviceId) }}
          </p>
          <HelpText>{{ messages.devices.pairingTrustHelp }}</HelpText>
          <label class="stack gap-2 text-sm">
            {{ step === 'accept' ? messages.devices.pairingInviteInput : messages.devices.pairingReplyInput }}
            <!--
              ★ The cap is the kernel's, not a round number. `synra-net`'s `MAX_INVITATION_BYTES = 2048`
              applies to the whole invitation string, and a reply is that budget plus its own prefix
              (2048 + 18 + 6 = 2072). One field takes both, so it uses the smaller: a reply inside 2048
              is still valid, while a 2200 cap let a 2150-character reply be typed in full and then
              refused by the kernel — the user pays for the round trip with nothing to show for it.
            -->
            <ATextarea v-model:value="imported" :maxlength="pairingInputLimit" :rows="3" :disabled="locked" />
          </label>
          <div>
            <AButton type="primary" :loading="pairing.busy" :disabled="cannotStart" @click="submit">
              {{ step === 'accept' ? messages.devices.pairingListen : messages.devices.pairingConnect }}
            </AButton>
          </div>
        </template>
      </template>
    </template>
    <p v-if="copyState === 'failed'" class="m-0 text-sm" role="alert">
      {{ messages.devices.pairingCopyFailed }}
    </p>
  </section>
</template>

<script setup lang="ts">
import type { DeviceCard } from '@synra/protocol'
import { Button as AButton, Select as ASelect, Textarea as ATextarea } from 'ant-design-vue'
import { computed, nextTick, onMounted, onScopeDispose, ref, watch } from 'vue'
import { formatDateTime } from '../locale-format'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'
import { usePairingStore } from '../stores/pairing'
import ErrorNotice from './ErrorNotice.vue'
import HelpText from './HelpText.vue'

const props = withDefaults(defineProps<{ selected?: DeviceCard | undefined, visible?: boolean }>(), { visible: true })
const emit = defineEmits<{ select: [deviceId: string] }>()
const devices = useDevicesStore()
const pairing = usePairingStore()
// Ant Design Vue exposes focus at runtime but its generated component instances omit it.
const stepControl = ref<{ focus: () => void }>()
const targetControl = ref<{ focus: () => void }>()
const invitationControl = ref<{ focus: () => void }>()
const taskStatus = ref<HTMLElement>()
let disposed = false
const step = ref<'create' | 'accept' | 'reply'>(props.selected ? 'accept' : 'create')

/**
 * How much text the import field accepts.
 *
 * Mirrors `synra_net::authentication::pairing::MAX_INVITATION_BYTES`, the kernel's own ceiling on the
 * whole invitation string. A reply is that budget plus `REPLY_PREFIX` and the port digits, so 2048 is
 * the smaller of the two and the safe cap for a field that takes either.
 */
const pairingInputLimit = 2048
const drafts = ref({ accept: '', reply: '' })
const imported = computed({
  get: () => step.value === 'reply' ? drafts.value.reply : drafts.value.accept,
  set: (value: string) => {
    if (step.value === 'reply')
      drafts.value.reply = value
    else
      drafts.value.accept = value
  }
})
const dismissedRequest = ref<string>()
const currentTask = computed(() => pairing.snapshot && (pairing.active || pairing.snapshot.requestId !== dismissedRequest.value) ? pairing.snapshot : null)
const taskName = computed(() => {
  const id = currentTask.value?.peerId ?? ''
  const name = devices.get(id)?.displayName
  return name ? `${name} (${id})` : id
})
const locked = computed(() => pairing.busy || pairing.active || pairing.refreshing || !!pairing.statusError)
const cannotStart = computed(() => locked.value || !props.selected || props.selected.paired || imported.value.trim().length === 0)
const targets = computed(() => devices.cards.filter(card => !card.paired).map(card => ({ value: card.deviceId, label: card.displayName })))
const steps = computed(() => [
  { value: 'create', label: messages.devices.pairingCreate },
  { value: 'accept', label: messages.devices.pairingAccept },
  { value: 'reply', label: messages.devices.pairingReplyStep }
])

function selectTarget(value: unknown): void {
  if (!locked.value && typeof value === 'string')
    emit('select', value)
}

async function submit(): Promise<void> {
  if (cannotStart.value || !props.selected)
    return
  if (step.value === 'accept')
    await pairing.listen(props.selected.deviceId, imported.value)
  else if (step.value === 'reply')
    await pairing.connect(props.selected.deviceId, imported.value)
  await nextTick()
  if (!disposed && props.visible !== false && currentTask.value)
    taskStatus.value?.focus()
}

async function generate(): Promise<void> {
  if (locked.value)
    return
  await pairing.exportInvitation()
  await nextTick()
  if (!disposed && props.visible !== false)
    invitationControl.value?.focus()
}

async function goToReply(): Promise<void> {
  if (locked.value)
    return
  step.value = 'reply'
  await nextTick()
  if (!disposed && props.visible !== false)
    targetControl.value?.focus()
}

async function restart(): Promise<void> {
  if (locked.value)
    return
  dismissedRequest.value = pairing.snapshot?.requestId
  pairing.resetDraft()
  drafts.value = { accept: '', reply: '' }
  step.value = 'create'
  await nextTick()
  if (!disposed && props.visible !== false)
    stepControl.value?.focus()
}

const copyState = ref<'idle' | 'pending' | 'copied' | 'failed'>('idle')
let copyRevision = 0
async function copy(value: string): Promise<void> {
  if (copyState.value === 'pending')
    return
  const revision = ++copyRevision
  copyState.value = 'pending'
  try {
    await navigator.clipboard.writeText(value)
    if (revision === copyRevision)
      copyState.value = 'copied'
  }
  catch {
    if (revision === copyRevision)
      copyState.value = 'failed'
  }
}

watch(() => props.selected?.deviceId, (id) => {
  drafts.value = { accept: '', reply: '' }
  if (id && step.value === 'create' && !pairing.active && !pairing.busy)
    step.value = 'accept'
})
watch(() => [step.value, pairing.invitation, currentTask.value?.reply, currentTask.value?.phase, props.selected?.deviceId], () => {
  copyRevision += 1
  copyState.value = 'idle'
})
onScopeDispose(() => {
  disposed = true
  copyRevision += 1
})
onMounted(() => {
  void pairing.refresh()
})
</script>
