<template>
  <section class="stack gap-4" :aria-label="messages.plugins.workflow.permissions">
    <div class="row-between gap-2">
      <h3 class="m-0 text-base font-medium">
        {{ messages.plugins.workflow.permissions }}
      </h3>
      <AButton v-if="!editing && capabilities.length" size="small" @click="editing = true">
        {{ messages.plugins.workflow.edit }}
      </AButton>
    </div>
    <p v-if="!capabilities.length" class="m-0 text-base text-ink-dim">
      {{ messages.plugins.preview.none }}
    </p>
    <div v-if="approvalOnly" class="stack items-start gap-3">
      <p class="m-0 text-sm text-ink-dim">
        {{ messages.plugins.workflow.noPermissionsApproval }}
      </p>
      <ErrorNotice v-if="error" :code="error.code" :detail="error.detail" :actions="error.actions?.filter(action => action.kind !== 'retry')" />
      <AButton type="primary" :loading="busy" :disabled="locked" @click="save">
        {{ messages.plugins.workflow.allowRun }}
      </AButton>
    </div>
    <div v-if="editing && capabilities.length" class="border-b border-line py-3">
      <ACheckbox
        data-testid="permission-all"
        class="w-full"
        :checked="allChecked"
        :indeterminate="allIndeterminate"
        :disabled="busy"
        @update:checked="selectAll"
      >
        <span class="text-base">{{ messages.plugins.workflow.all }}</span>
      </ACheckbox>
    </div>
    <div v-for="capability in capabilities" :key="capability" :data-permission-capability="editing ? capability : undefined" class="min-w-0 border-b border-line py-3">
      <ACheckbox
        v-if="editing"
        class="w-full"
        :checked="draft.includes(capability)"
        :disabled="busy"
        @update:checked="checked => select(capability, checked)"
      >
        <span class="min-w-0 stack gap-1">
          <span class="text-base">{{ capabilityPurpose(capability) }}</span>
          <code class="break-all text-sm text-ink-dim">{{ capability }}</code>
        </span>
      </ACheckbox>
      <div v-else class="row-between items-start gap-3">
        <span class="min-w-0 stack gap-1">
          <span class="text-base">{{ capabilityPurpose(capability) }}</span>
          <code class="break-all text-sm text-ink-dim">{{ capability }}</code>
        </span>
        <span class="shrink-0 text-sm text-ink-dim">{{ plugin.granted.includes(capability) ? messages.plugins.workflow.allowed : messages.plugins.workflow.denied }}</span>
      </div>
      <span v-if="editing && draft.includes(capability) !== plugin.granted.includes(capability)" class="text-sm text-primary-ink">{{ messages.plugins.workflow.changed }}</span>
      <span v-else-if="!plugin.granted.includes(capability) && !denied.includes(capability)" class="text-sm text-ink-dim">{{ messages.plugins.workflow.newPermission }}</span>
    </div>
    <template v-if="editing && capabilities.length">
      <p class="m-0 text-sm text-ink-dim">
        {{ messages.plugins.workflow.permissionEffect }}
      </p>
      <ErrorNotice v-if="error" :code="error.code" :detail="error.detail" :actions="error.actions?.filter(action => action.kind !== 'retry')" />
      <div class="row justify-end gap-2">
        <AButton :disabled="busy" @click="cancel">
          {{ messages.plugins.workflow.cancel }}
        </AButton>
        <AButton type="primary" :loading="busy" :disabled="locked || (!changed && plugin.state !== 'awaiting_consent' && !firstDecision)" @click="save">
          {{ firstDecision || plugin.state === 'awaiting_consent' ? messages.plugins.workflow.allowSelected : messages.plugins.consentAction }}
        </AButton>
      </div>
    </template>
  </section>
</template>

<script setup lang="ts">
import type { PluginSnapshot } from '@synra/protocol'
import type { Failure } from '../../features/plugins/actions'
import type { usePluginOperations } from '../../features/plugins/operations'
import { Button as AButton, Checkbox as ACheckbox } from 'ant-design-vue'
import { computed, ref, watch } from 'vue'
import { kernel } from '../../app/kernel'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { decide } from '../../features/plugins/actions'
import { capabilityPurpose } from '../../features/plugins/capabilities'
import { messages } from '../../messages'
import { usePluginsStore } from '../../stores/plugins'

const props = defineProps<{ plugin: PluginSnapshot, operations: ReturnType<typeof usePluginOperations>, initialEdit?: boolean }>()
const store = usePluginsStore()
const draft = ref<string[]>([...props.plugin.granted])
const editing = ref(props.initialEdit || props.plugin.state === 'awaiting_consent')
const error = ref<Failure>()
const busy = computed(() => props.operations.busy.get(props.plugin.pluginId) === 'consent')
const locked = computed(() => props.operations.busy.has(props.plugin.pluginId))
const capabilities = computed(() => [...new Set(props.plugin.requested ?? [])])
const denied = computed(() => props.plugin.denied ?? [])
const approvalOnly = computed(() => !capabilities.value.length && (props.plugin.state === 'awaiting_consent' || (props.initialEdit && editing.value)))
const firstDecision = computed(() => !props.plugin.granted.length && !denied.value.length)
const allChecked = computed(() => capabilities.value.length > 0 && capabilities.value.every(capability => draft.value.includes(capability)))
const allIndeterminate = computed(() => !allChecked.value && capabilities.value.some(capability => draft.value.includes(capability)))
const same = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every(value => b.includes(value))
const changed = computed(() => !same(draft.value, props.plugin.granted))
watch(() => props.plugin, (next, previous) => {
  if (same(draft.value, previous.granted))
    draft.value = [...next.granted]
})

function select(capability: string, checked: boolean): void {
  draft.value = checked ? [...new Set([...draft.value, capability])] : draft.value.filter(value => value !== capability)
}
function selectAll(checked: boolean): void {
  draft.value = checked ? [...capabilities.value] : []
}
function cancel(): void {
  draft.value = [...props.plugin.granted]
  error.value = undefined
  editing.value = false
}
async function save(): Promise<void> {
  error.value = undefined
  const answer = await props.operations.run(props.plugin.pluginId, 'consent', () => decide(kernel, props.plugin.pluginId, draft.value.filter(value => capabilities.value.includes(value))))
  if (!answer)
    return
  if (!answer.ok) {
    error.value = answer.failure
    return
  }
  store.upsert(answer.value)
  draft.value = [...answer.value.granted]
  editing.value = false
}
</script>
