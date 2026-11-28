<template>
  <ADrawer :open="visible" :push="false" width="min(600px, 100vw)" :title="messages.plugins.publishers.title" @close="$emit('close')">
    <div class="stack gap-4">
      <p class="text-base text-ink-dim m-0">
        {{ messages.plugins.publishers.detail }}
      </p>
      <ErrorNotice v-if="model.error" :code="model.error.code" :actions="model.error.actions" @retry="model.read" />
      <p v-if="!model.list.length" class="text-base text-ink-dim m-0">
        {{ messages.plugins.publishers.empty }}
      </p>
      <div v-for="publisher in model.list" :key="publisher.publisherId" class="pb-3 border-b border-line stack gap-2 min-w-0">
        <div class="row-between gap-2">
          <span class="text-base break-all">{{ publisher.publisherId }}</span><span class="text-sm text-ink-dim">{{ messages.plugins.publishers.trustStates[publisher.trust as keyof typeof messages.plugins.publishers.trustStates] ?? publisher.trust }}</span>
        </div>
        <code class="text-sm text-ink-dim select-text break-all">{{ publisher.fingerprint }}</code>
        <div class="row gap-2">
          <AButton v-if="publisher.trust !== 'trusted'" size="small" :loading="model.deciding === publisher.publisherId" :disabled="!!model.deciding" @click="model.decide(publisher.publisherId, publisher.fingerprint, 'trusted')">
            {{ messages.plugins.publishers.trust }}
          </AButton>
          <AButton v-if="publisher.trust !== 'revoked'" size="small" :loading="model.deciding === publisher.publisherId" :disabled="!!model.deciding" @click="model.decide(publisher.publisherId, publisher.fingerprint, 'revoked')">
            {{ messages.plugins.publishers.revoke }}
          </AButton>
        </div>
        <ErrorNotice v-if="model.errors[publisher.publisherId]" :code="model.errors[publisher.publisherId]!.code" :actions="model.errors[publisher.publisherId]!.actions" />
      </div>
    </div>
  </ADrawer>
</template>

<script setup lang="ts">
import type { Publishers } from '../../features/plugins/publishers'
import { Button as AButton, Drawer as ADrawer } from 'ant-design-vue'
import { ref } from 'vue'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { messages } from '../../messages'
import { useDialogVisibility } from '../../stores/dialogs'

defineProps<{ model: Publishers }>()
const emit = defineEmits<{ close: [] }>()
const visible = useDialogVisibility(ref(true), 30, () => emit('close'))
</script>
