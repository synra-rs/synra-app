<template>
  <ADrawer :get-container="false" :open="visible" width="min(640px, 100vw)" :title="plugin.title" :closable="!busy" @close="close">
    <div class="stack gap-4">
      <div class="row-between flex-wrap gap-2">
        <span class="text-base text-ink-dim">{{ messages.plugins.states[plugin.state as keyof typeof messages.plugins.states] ?? plugin.state }}</span>
        <div class="row gap-2">
          <AButton v-if="plugin.hasUi" :disabled="busy" @click="library.open(plugin)">
            {{ messages.plugins.openAction }}
          </AButton>
          <PluginActionMenu :plugin="plugin" :disabled="busy" @action="action" />
        </div>
      </div>
      <ErrorNotice v-if="library.errors[plugin.pluginId]" :code="library.errors[plugin.pluginId]!.code" :detail="library.errors[plugin.pluginId]!.detail" :actions="library.errors[plugin.pluginId]!.actions" />
      <ATabs v-model:active-key="tab">
        <ATabPane key="overview" :tab="messages.plugins.workflow.overview">
          <PluginOverview :plugin="plugin" />
          <AButton v-if="plugin.state === 'faulted'" class="mt-4" :disabled="busy" @click="library.retry(plugin)">
            {{ messages.plugins.retry }}
          </AButton>
        </ATabPane>
        <ATabPane key="permissions" :tab="messages.plugins.workflow.permissions">
          <PluginPermissions :plugin="plugin" :operations="library.operations" :initial-edit="initialAction === 'review'" />
        </ATabPane>
        <ATabPane key="audit" :tab="messages.plugins.audit.title">
          <PluginAudit :plugin-id="plugin.pluginId" :active="tab === 'audit' && visible" />
        </ATabPane>
      </ATabs>
      <PluginSync ref="sync" triggerless :plugin="plugin" :operations="library.operations" />
    </div>
  </ADrawer>
  <PluginRemoval v-if="removing" :plugin="plugin" :library="library" @close="removed" />
</template>

<script setup lang="ts">
import type { PluginSnapshot } from '@synra/protocol'
import type { PluginLibrary } from './library'
import type { PluginMenuAction } from './menu'
import { Button as AButton, Drawer as ADrawer, TabPane as ATabPane, Tabs as ATabs } from 'ant-design-vue'
import { computed, nextTick, ref, watch } from 'vue'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { messages } from '../../messages'
import { useDialogVisibility } from '../../stores/dialogs'
import { usePluginsStore } from '../../stores/plugins'
import PluginActionMenu from './PluginActionMenu.vue'
import PluginAudit from './PluginAudit.vue'
import PluginOverview from './PluginOverview.vue'
import PluginPermissions from './PluginPermissions.vue'
import PluginRemoval from './PluginRemoval.vue'
import PluginSync from './PluginSync.vue'

const props = defineProps<{ plugin: PluginSnapshot, library: PluginLibrary, initialTab?: string, initialAction?: string }>()
const emit = defineEmits<{ close: [] }>()
const tab = ref(props.initialTab || 'overview')
watch(() => props.initialTab, value => tab.value = value || 'overview')
const removing = ref(false)
const sync = ref<InstanceType<typeof PluginSync>>()
watch(() => props.initialAction, async (value) => {
  if (value === 'sync') {
    await nextTick()
    void sync.value?.show()
  }
}, { immediate: true })
const visible = useDialogVisibility(ref(true), 20)
const busy = computed(() => props.library.operations.busy.has(props.plugin.pluginId))
function close(): void {
  if (!busy.value)
    emit('close')
}
function action(key: PluginMenuAction): void {
  switch (key) {
    case 'check':
      void props.library.checkUpdate(props.plugin)
      break
    case 'update':
      void props.library.update(props.plugin)
      break
    case 'sync':
      void sync.value?.show()
      break
    case 'audit':
      tab.value = 'audit'
      break
    case 'remove':
      removing.value = true
      break
  }
}
const store = usePluginsStore()
function removed(): void {
  removing.value = false
  if (!store.plugin(props.plugin.pluginId))
    emit('close')
}
</script>
