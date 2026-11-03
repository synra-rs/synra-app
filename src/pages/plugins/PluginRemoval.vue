<template>
  <AModal :get-container="false" :open="visible" :width="440" :title="messages.plugins.removeTitle" :closable="!busy" :mask-closable="!busy" :keyboard="!busy" :footer="null" @cancel="$emit('close')">
    <div class="stack gap-4">
      <p class="m-0 text-base">
        {{ messages.plugins.removeDetail(plugin.title) }}
      </p>
      <p class="m-0 break-all text-sm text-ink-dim font-mono">
        {{ messages.plugins.removeDataPath(plugin.dataPath) }}
      </p>
      <ACheckbox v-model:checked="removeData" :disabled="busy">
        {{ messages.plugins.removeDeleteData }}
      </ACheckbox>
      <ErrorNotice v-if="library.errors[plugin.pluginId]" :code="library.errors[plugin.pluginId]!.code" :actions="library.errors[plugin.pluginId]!.actions?.filter(action => action.kind !== 'retry')" />
      <div class="row justify-end gap-2">
        <AButton :disabled="busy" @click="$emit('close')">
          {{ messages.plugins.workflow.cancel }}
        </AButton>
        <AButton type="primary" danger :loading="busy" @click="remove">
          {{ messages.plugins.removeAction }}
        </AButton>
      </div>
    </div>
  </AModal>
</template>

<script setup lang="ts">
import type { PluginSnapshot } from '@synra/protocol'
import type { PluginLibrary } from './library'
import { Button as AButton, Checkbox as ACheckbox, Modal as AModal } from 'ant-design-vue'
import { computed, ref } from 'vue'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { messages } from '../../messages'
import { useDialogVisibility } from '../../stores/dialogs'

const props = defineProps<{ plugin: PluginSnapshot, library: PluginLibrary }>()
const emit = defineEmits<{ close: [] }>()
const removeData = ref(false)
const busy = computed(() => props.library.operations.busy.has(props.plugin.pluginId))
const visible = useDialogVisibility(ref(true), 60)
async function remove(): Promise<void> {
  if (await props.library.remove(props.plugin, removeData.value))
    emit('close')
}
</script>
