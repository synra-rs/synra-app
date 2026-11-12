<template>
  <AModal :open="visible" :width="560" :title="messages.plugins.workflow.installConsentTitle" :closable="!busy" :mask-closable="!busy" :keyboard="!busy" :footer="null" @cancel="$emit('close')">
    <div class="stack gap-4">
      <div class="stack gap-1">
        <h3 class="m-0 text-lg font-semibold">
          {{ plugin.title }}
        </h3>
        <p v-if="plugin.description" class="m-0 text-base">
          {{ plugin.description }}
        </p>
        <p class="m-0 break-all text-sm text-ink-dim font-mono">
          {{ plugin.pluginId }} · {{ plugin.version }} · {{ plugin.publisher }}
        </p>
      </div>
      <div class="stack gap-2">
        <h4 class="m-0 text-base font-medium">
          {{ messages.plugins.workflow.requestedPermissions }}
        </h4>
        <div v-for="capability in capabilities" :key="capability" class="stack gap-1 border-b border-line pb-2">
          <span class="text-base">{{ capabilityPurpose(capability) }}</span>
          <code class="break-all text-sm text-ink-dim">{{ capability }}</code>
        </div>
        <p v-if="!capabilities.length" class="m-0 text-base text-ink-dim">
          {{ messages.plugins.preview.none }}
        </p>
      </div>
      <ErrorNotice v-if="error" :code="error.code" :detail="error.detail" :actions="error.actions?.filter(action => action.kind !== 'retry')" />
      <div class="row justify-end gap-2">
        <AButton danger :disabled="busy" @click="reject">
          {{ messages.plugins.workflow.rejectInstall }}
        </AButton>
        <AButton type="primary" :loading="busy" @click="accept">
          {{ messages.plugins.workflow.acceptInstall }}
        </AButton>
      </div>
    </div>
  </AModal>
</template>

<script setup lang="ts">
import type { PluginSnapshot } from '@synra/protocol'
import type { Failure } from '../../features/plugins/actions'
import type { PluginLibrary } from '../../features/plugins/library'
import { Button as AButton, Modal as AModal } from 'ant-design-vue'
import { computed, ref } from 'vue'
import { kernel } from '../../app/kernel'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { decide } from '../../features/plugins/actions'
import { capabilityPurpose } from '../../features/plugins/capabilities'
import { messages } from '../../messages'
import { useDialogVisibility } from '../../stores/dialogs'
import { usePluginsStore } from '../../stores/plugins'

const props = defineProps<{ plugin: PluginSnapshot, library: PluginLibrary }>()
const emit = defineEmits<{ close: [] }>()
const store = usePluginsStore()
const visible = useDialogVisibility(ref(true), 80)
const capabilities = computed(() => [...new Set(props.plugin.requested ?? [])])
const busy = computed(() => props.library.operations.busy.has(props.plugin.pluginId))
const error = ref<Failure>()

async function accept(): Promise<void> {
  error.value = undefined
  const answer = await props.library.operations.run(props.plugin.pluginId, 'consent', () => decide(kernel, props.plugin.pluginId, capabilities.value))
  if (!answer)
    return
  if (!answer.ok) {
    error.value = answer.failure
    return
  }
  store.upsert(answer.value)
  emit('close')
}

async function reject(): Promise<void> {
  error.value = undefined
  const removed = await props.library.remove(props.plugin, false)
  if (removed) {
    emit('close')
  }
}
</script>
