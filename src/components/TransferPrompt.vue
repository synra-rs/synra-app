<style scoped>
.transfer-prompt {
  top: max(1rem, env(safe-area-inset-top));
  right: max(1rem, env(safe-area-inset-right));
  width: min(24rem, calc(100vw - 2rem - env(safe-area-inset-left) - env(safe-area-inset-right)));
  max-height: calc(100dvh - 2rem - env(safe-area-inset-top) - env(safe-area-inset-bottom));
}

@media (pointer: coarse), (max-width: 639px) {
  .transfer-prompt :deep(.ant-btn),
  .transfer-prompt :deep(.ant-checkbox-wrapper) {
    min-height: 44px;
  }
  .transfer-prompt :deep(.ant-checkbox-wrapper) {
    align-items: center;
  }
  .transfer-prompt summary {
    min-height: 44px;
    align-content: center;
  }
}
</style>

<template>
  <section v-if="prompt" ref="panel" class="transfer-prompt ring-warning fixed z-50 min-w-0 stack gap-3 overflow-y-auto overscroll-contain rounded bg-surface p-4 shadow-lg ring-1" aria-labelledby="transfer-prompt-title" tabindex="-1">
    <div class="stack gap-1" role="status" aria-live="polite" aria-atomic="true">
      <h2 id="transfer-prompt-title" class="m-0 text-sm font-medium">
        {{ messages.transfers.prompt.title }}
      </h2>
      <span v-if="offered.length > 1" class="text-xs text-ink-dim">{{ messages.transfers.prompt.pending(offered.length) }}</span>
      <span class="break-words text-sm font-medium">{{ prompt.peerName }}</span>
      <span class="line-clamp-2 break-all text-sm">{{ prompt.fileName }}</span>
    </div>
    <details class="text-xs text-ink-dim">
      <summary class="min-h-11 cursor-pointer">
        {{ messages.transfers.prompt.details }}
      </summary>
      <dl class="m-0 stack gap-1 pt-2">
        <dt>{{ messages.transfers.prompt.fileName }}</dt>
        <dd class="m-0 break-all">
          {{ prompt.fileName }}
        </dd>
        <dt>{{ messages.transfers.prompt.deviceId }}</dt>
        <dd class="m-0 break-all font-mono">
          {{ prompt.peerDevice }}
        </dd>
        <dt>{{ messages.transfers.prompt.transferId }}</dt>
        <dd class="m-0 break-all font-mono">
          {{ prompt.transferId }}
        </dd>
      </dl>
    </details>
    <p class="m-0 text-xs text-ink-dim">
      {{ messages.transfers.prompt.detail }}
    </p>
    <ACheckbox v-model:checked="always" :disabled="busy || answered">
      {{ messages.transfers.prompt.always }}
    </ACheckbox>
    <ErrorNotice v-if="replyError" :code="replyError.code" :actions="withoutRetry(replyError.actions)" />
    <div v-if="!answered" class="row flex-wrap gap-2">
      <AButton type="primary" :loading="pending?.id === prompt.transferId && pending.accept" :disabled="busy" @click="answer(true)">
        {{ messages.transfers.prompt.accept }}
      </AButton>
      <AButton ref="rejectButton" :loading="pending?.id === prompt.transferId && !pending.accept" :disabled="busy" @click="answer(false)">
        {{ messages.transfers.prompt.reject }}
      </AButton>
    </div>
    <p v-else class="m-0 text-xs text-ink-dim" role="status">
      {{ messages.transfers.prompt.answered }}
    </p>
    <div class="stack gap-2">
      <ErrorNotice v-if="statusError" :code="statusError.code" :actions="withoutRetry(statusError.actions)" />
      <AButton v-if="replyError || statusError || answered" :loading="reading" :disabled="busy || reading" @click="checkStatus">
        {{ messages.transfers.prompt.checkStatus }}
      </AButton>
    </div>
  </section>
</template>

<script setup lang="ts">
import type { IpcError, SuggestedAction } from '@synra/protocol'
import { Button as AButton, Checkbox as ACheckbox } from 'ant-design-vue'
import { computed, nextTick, onUnmounted, ref, watch } from 'vue'
import { kernel } from '../kernel'
import { messages } from '../messages'
import { useIntentsStore } from '../stores/intents'
import ErrorNotice from './ErrorNotice.vue'

const intents = useIntentsStore()
const offered = computed(() => intents.transfers.filter(entry => entry.direction === 'receive' && entry.state === 'offered'))
const prompt = computed(() => offered.value[0])
const pending = ref<{ id: string, accept: boolean }>()
const busy = computed(() => pending.value !== undefined)
const always = ref(false)
const answeredId = ref<string>()
const answered = computed(() => answeredId.value === prompt.value?.transferId && answeredId.value !== undefined)
const replyError = ref<IpcError>()
const statusError = ref<IpcError>()
const reading = ref(false)
const panel = ref<HTMLElement>()
const rejectButton = ref<InstanceType<typeof AButton>>()
let disposed = false

function withoutRetry(actions?: readonly SuggestedAction[]): readonly SuggestedAction[] {
  return actions?.filter(action => action.kind !== 'retry') ?? []
}

watch(() => prompt.value?.transferId, (id) => {
  const hadFocus = panel.value?.contains(document.activeElement)
  always.value = false
  answeredId.value = undefined
  replyError.value = undefined
  statusError.value = undefined
  if (id && hadFocus) {
    void nextTick(() => {
      if (!disposed && prompt.value?.transferId === id) {
        if (busy.value)
          panel.value?.focus()
        else
          rejectButton.value?.$el?.focus()
      }
    })
  }
}, { flush: 'sync' })

async function answer(accept: boolean): Promise<void> {
  const current = prompt.value
  if (!current || busy.value || answered.value)
    return
  pending.value = { id: current.transferId, accept }
  replyError.value = undefined
  try {
    const result = await kernel.call<null>('transfer_respond', {
      transferId: current.transferId,
      accept,
      remember: accept && always.value
    })
    if (disposed || prompt.value?.transferId !== current.transferId)
      return
    if (result.ok) {
      answeredId.value = current.transferId
      always.value = false
    }
    else {
      replyError.value = result.error
    }
  }
  finally {
    pending.value = undefined
  }
}

async function checkStatus(): Promise<void> {
  const id = prompt.value?.transferId
  if (!id || reading.value || busy.value)
    return
  reading.value = true
  try {
    const result = await intents.readTransfers(kernel)
    if (!disposed && prompt.value?.transferId === id)
      statusError.value = result.ok ? undefined : result.error
  }
  finally {
    reading.value = false
  }
}

onUnmounted(() => {
  disposed = true
})
</script>
