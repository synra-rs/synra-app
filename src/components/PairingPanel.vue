<style scoped>
/*
 * ★ `09 §3` sets 44px as the floor for a thumb — the same number `min-h-11` carries (11 × 4px spacing) and the one
 *   `App.vue`'s bottom bar restates. Three places, one number, no constant shared between them: the utility has it
 *   baked in, and these two rules exist because a `<select>` height and a `<summary>` are not buttons.
 *
 * ★ 639 rather than 767: `03 §3`'s narrow layout is `<640`, and the utility route drops back to 32px at `md` (768).
 *   Ending here keeps every surface on one screen agreeing about which layout it is in —
 *   `check-shapes.ts`'s `touch-breakpoints` holds the number.
 */
@media (max-width: 639px), (pointer: coarse) {
  .pairing-panel :deep(.ant-btn) {
    min-height: 44px;
  }

  .pairing-panel :deep(.ant-select-single) {
    height: 44px;
  }
}
</style>

<template>
  <section class="pairing-panel stack gap-4 border border-line rounded-surface border-solid p-4" aria-labelledby="pairing-title">
    <h2 id="pairing-title" class="m-0 text-base font-medium">
      {{ messages.devices.pairingTitle }}
    </h2>
    <!--
      ★ The requests another device is waiting on this person to decide about.

      This is the half that never existed: a device asked, the kernel filed the request and blocked the connection, and
      nothing ever put the question in front of the only person who could answer it. Above the invitation flow because it
      arrives on its own — a person who opened this panel to pair outward should still see that someone is asking.

      Not a modal: the list can hold several, and a request that blocks the rest of the page would make the second one
      unreachable. Each row carries its own answer, and a row is only removed when the kernel says it was answered.
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
        <!-- The full id, because two devices can share a name and the name is only what the other one claimed. -->
        <p class="m-0 break-all font-mono text-xs text-ink-dim">
          {{ prompt.deviceId }}
        </p>
        <!--
          The code is the verification: the other device shows the same six characters, and the person compares them.
          Stated as a comparison rather than as an identifier, because it proves the two screens are looking at the same
          request and nothing more.
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
          <!-- 44px where a finger is the pointer, antd's own floor from `md` up — same rule as every other surface. -->
          <AButton
            type="primary"
            class="min-h-11 md:min-h-8"
            :loading="pairing.answering === prompt.requestId"
            :disabled="pairing.answering !== null && pairing.answering !== prompt.requestId"
            @click="pairing.answer(prompt.requestId, true)"
          >
            {{ messages.devices.pairingRequestAccept }}
          </AButton>
          <AButton
            class="min-h-11 md:min-h-8"
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
      <ErrorNotice v-if="currentTask.errorCode" :code="currentTask.errorCode" />
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
/**
 * How often the waiting-request list is read.
 *
 * Three seconds: a pairing window is tens of seconds, so noticing a second late costs a second of a person's wait. Every
 * frame would spend a round trip per frame to shorten that, on a list that is usually empty.
 */
const REQUEST_POLL_MS = 3000

onMounted(() => {
  void pairing.refresh()
  /*
   * ★ A request arrives on a socket thread; nothing announces it to this page. So the list is polled.
   *
   * Three seconds, not every frame: a pairing window is tens of seconds, and the cost of noticing a second late is that
   * a person reads the prompt a second late. Polling every frame would spend a kernel round trip per frame to make that
   * second shorter, and the desk's list is small enough that the plain read is the cheap half already.
   *
   * On the scope, not the component: a page change disposes it without a second `onBeforeUnmount` to keep in step — and
   * `onScopeDispose` is what `watch` above already uses for the copy revision.
   */
  const poll = setInterval(() => {
    void pairing.refreshPending()
  }, REQUEST_POLL_MS)
  onScopeDispose(() => clearInterval(poll))
  void pairing.refreshPending()
})
</script>
