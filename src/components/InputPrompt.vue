<template>
  <AModal v-if="pending" :open="visible" :title="messages.input.title" :footer="null" :closable="false" :mask-closable="false" :width="420">
    <div class="stack gap-4">
      <p class="m-0 text-base">
        {{ messages.input.request(session!.displayName) }}
      </p>
      <p class="m-0 text-sm text-ink-dim">
        {{ messages.input.help }}
      </p>
      <p class="m-0 break-all text-sm text-ink-dim">
        {{ messages.input.plugin }} · {{ session!.pluginId }}
      </p>
      <ErrorNotice v-if="input.error" :code="input.error.code" :params="input.error.params" />
      <div class="row justify-end gap-2">
        <AButton :disabled="input.busy" @click="input.decide(false)">
          {{ messages.input.refuse }}
        </AButton>
        <AButton type="primary" :loading="input.busy" @click="input.decide(true)">
          {{ messages.input.allow }}
        </AButton>
      </div>
    </div>
  </AModal>
  <aside v-if="session?.phase === 'active' || session?.phase === 'starting'" role="status" :aria-label="messages.input.indicator" class="fixed bottom-20 left-4 right-4 z-90 border border-line rounded-surface bg-raised p-3 shadow-lg md:bottom-4 md:left-auto md:max-w-md">
    <div class="row flex-wrap justify-between gap-3">
      <span class="text-sm">{{ session.phase === 'active' ? messages.input.active(session.displayName) : messages.input.starting }}</span>
      <AButton danger size="small" :loading="input.busy" @click="input.stop">
        {{ messages.input.stop }}
      </AButton>
    </div>
    <ErrorNotice v-if="input.error" :code="input.error.code" :params="input.error.params" />
  </aside>
</template>

<script setup lang="ts">
import { Button as AButton, Modal as AModal } from 'ant-design-vue'
import { computed } from 'vue'
import { messages } from '../messages'
import { useDialogVisibility } from '../stores/dialogs'
import { useInputStore } from '../stores/input'
import ErrorNotice from './ErrorNotice.vue'

const input = useInputStore()
const session = computed(() => input.snapshot.session)
const pending = computed(() => session.value?.phase === 'pending')
const visible = useDialogVisibility(pending, 97)
</script>
