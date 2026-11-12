<template>
  <AModal :open="visible" :width="440" :title="messages.plugins.workflow.reloadTitle" :closable="!busy" :mask-closable="!busy" :keyboard="!busy" :footer="null" @cancel="$emit('close')">
    <div class="stack gap-4">
      <p class="m-0 text-base">
        {{ messages.plugins.workflow.reloadDetail(plugin.title) }}
      </p>
      <p class="m-0 break-all text-sm text-ink-dim font-mono">
        {{ messages.plugins.workflow.reloadSource(plugin.pluginId) }}
      </p>
      <ACheckbox v-model:checked="removeData" :disabled="busy">
        {{ messages.plugins.removeDeleteData }}
      </ACheckbox>
      <ErrorNotice v-if="error" :code="error.code" :detail="error.detail" :actions="error.actions?.filter(action => action.kind !== 'retry')" />
      <div class="row justify-end gap-2">
        <AButton :disabled="busy" @click="$emit('close')">
          {{ messages.plugins.workflow.cancel }}
        </AButton>
        <AButton type="primary" :loading="busy" @click="reload">
          {{ messages.plugins.workflow.reloadAction }}
        </AButton>
      </div>
    </div>
  </AModal>
</template>

<script setup lang="ts">
import type { PluginSnapshot } from '@synra/protocol'
import type { Failure } from '../../features/plugins/actions'
import type { PluginLibrary } from '../../features/plugins/library'
import { Button as AButton, Checkbox as ACheckbox, Modal as AModal } from 'ant-design-vue'
import { computed, ref } from 'vue'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { messages } from '../../messages'
import { useDialogVisibility } from '../../stores/dialogs'

const props = defineProps<{ plugin: PluginSnapshot, library: PluginLibrary }>()
const emit = defineEmits<{ close: [] }>()
const visible = useDialogVisibility(ref(true), 70)
const removeData = ref(false)
const error = ref<Failure>()
const busy = computed(() => props.library.operations.busy.has(props.plugin.pluginId))

async function reload(): Promise<void> {
  error.value = undefined
  if (await props.library.reloadLocal(props.plugin, removeData.value))
    emit('close')
  else
    error.value = props.library.errors[props.plugin.pluginId]
}
</script>
