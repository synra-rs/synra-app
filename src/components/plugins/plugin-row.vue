<template>
  <article class="min-w-0 stack gap-2 border-b border-line py-3" :data-plugin="plugin.pluginId">
    <div class="min-w-0 row flex-wrap gap-3">
      <button type="button" class="synra-choice min-w-0 flex-1 border-0 rounded-control bg-transparent p-0 text-start text-ink focusable" @click="$emit('details')">
        <span class="block truncate text-base font-medium">{{ plugin.title }}</span>
        <span class="block truncate text-sm text-ink-dim">{{ messages.plugins.states[plugin.state as keyof typeof messages.plugins.states] ?? plugin.state }} · {{ summary }}</span>
      </button>
      <AButton v-if="plugin.state === 'awaiting_consent'" size="small" @click="$emit('details', 'permissions')">
        {{ messages.plugins.workflow.installConsentTitle }}
      </AButton>
      <AButton v-else-if="plugin.state === 'faulted'" size="small" :disabled="busy" @click="library.retry(plugin)">
        {{ messages.plugins.retry }}
      </AButton>
      <AButton v-if="plugin.hasUi && plugin.state !== 'awaiting_consent'" size="small" :disabled="busy" :aria-label="messages.plugins.openLabel(plugin.title)" @click="library.open(plugin)">
        {{ messages.plugins.openAction }}
      </AButton>
      <span class="synra-choice row shrink-0">
        <ASwitch :aria-label="messages.plugins.enableLabel(plugin.title)" :checked="plugin.enabled" :loading="library.operations.busy.get(plugin.pluginId) === 'toggle'" :disabled="busy" @change="value => library.toggle(plugin, value as boolean)" />
      </span>
      <PluginActionMenu compact :plugin="plugin" :disabled="busy" @action="action => $emit('action', action)" />
    </div>
    <p v-if="plugin.update" class="m-0 text-sm text-primary-ink">
      {{ plugin.update.needsNewConsent ? messages.plugins.updateNeedsConsent : messages.plugins.updateAvailable }}
    </p>
    <p v-if="attention" class="m-0 text-sm text-warn">
      {{ messages.plugins.publishers.untrusted }}
    </p>
    <ErrorNotice v-if="library.errors[plugin.pluginId]" :code="library.errors[plugin.pluginId]!.code" :detail="library.errors[plugin.pluginId]!.detail" :actions="library.errors[plugin.pluginId]!.actions" />
  </article>
</template>

<script setup lang="ts">
import type { PluginSnapshot } from '@synra/protocol'
import type { PluginLibrary } from '../../features/plugins/library'
import type { PluginMenuAction } from '../../features/plugins/menu'
import { Button as AButton, Switch as ASwitch } from 'ant-design-vue'
import { computed } from 'vue'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { messages } from '../../messages'
import PluginActionMenu from './plugin-action-menu.vue'

const props = defineProps<{ plugin: PluginSnapshot, library: PluginLibrary, attention?: boolean }>()
defineEmits<{ details: [tab?: 'permissions'], action: [action: PluginMenuAction] }>()
const busy = computed(() => props.library.operations.busy.has(props.plugin.pluginId))
const summary = computed(() => props.plugin.stateReason || (props.plugin.health && props.plugin.health.health !== 'good' ? messages.plugins.healthLabel(props.plugin.health.health) : props.plugin.description || props.plugin.pluginId))
</script>
