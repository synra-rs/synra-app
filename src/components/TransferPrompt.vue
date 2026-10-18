<template>
  <!--
    ★ A prompt, not a notification: the peer's connection is held open until this is answered, and the file is not
    stored until then. It lives at the root rather than on the home page because a request arrives on its own schedule —
    a person reading the plugins page is exactly as able to answer it, and a prompt that vanished on navigation would
    leave the peer waiting out the window.
  -->
  <div
    v-if="prompt"
    class="ring-warning fixed right-4 top-4 z-50 w-80 stack gap-2 rounded bg-surface p-3 shadow-lg ring-1"
    role="alert"
  >
    <span class="text-sm font-medium">{{ messages.transfers.prompt.title }}</span>
    <span class="text-xs text-ink-dim">{{ messages.transfers.prompt.detail }}</span>
    <span class="truncate text-sm">
      <span class="font-medium">{{ prompt.peerName }}</span>
      <span class="text-ink-dim"> · </span>
      <span class="font-mono">{{ prompt.fileName }}</span>
    </span>
    <!--
      The preference is applied to **accept** only: a refusal says something about this file, not about the device. The
      checkbox is not pre-checked, because silence is not a policy.
    -->
    <ACheckbox v-model:checked="always">
      {{ messages.transfers.prompt.always }}
    </ACheckbox>
    <div class="row gap-2">
      <AButton size="small" :loading="busy" @click="answer(true)">
        {{ messages.transfers.prompt.accept }}
      </AButton>
      <AButton size="small" :disabled="busy" @click="answer(false)">
        {{ messages.transfers.prompt.reject }}
      </AButton>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { TransferSnapshot } from '@synra/protocol'
import { Button as AButton, Checkbox as ACheckbox } from 'ant-design-vue'
import { storeToRefs } from 'pinia'
import { computed, ref } from 'vue'

import { kernel } from '../kernel'
import { messages } from '../messages'
import { useIntentsStore } from '../stores/intents'

const intents = useIntentsStore()
const { transfers } = storeToRefs(intents)

/**
 * One prompt at a time.
 *
 * The desk answers one question per transfer, and a stack of prompts would ask a person to make the same decision twice
 * while a peer waits. The rest stay in the list until they are answered or their window closes.
 */
const prompt = computed<TransferSnapshot | undefined>(() =>
  transfers.value.find(entry => entry.state === 'offered')
)

/** The answer is in flight; a second click would be a second answer to a question that is being settled. */
const busy = ref(false)

/** Whether this answer should also silence later prompts from the same device. */
const always = ref(false)

async function answer(accept: boolean): Promise<void> {
  const current = prompt.value
  if (!current || busy.value)
    return
  busy.value = true
  // The row leaves through `transferFinished`, which the kernel publishes when the waiting connection has its answer —
  // not from here, where a failed command would remove a prompt the user still has to answer.
  await kernel.call<null>('transfer_respond', {
    transferId: current.transferId,
    accept,
    remember: accept && always.value
  })
  // The next prompt starts unchecked: a preference someone set for one device is not a default for every device.
  always.value = false
  busy.value = false
}
</script>
