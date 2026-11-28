<style scoped>
.plugin-actions {
  display: flex;
  min-width: 13rem;
  min-height: 2rem;
  flex: 0 0 auto;
  flex-wrap: wrap;
  gap: 0.5rem;
  align-items: center;
  justify-content: flex-end;
  opacity: 0;
  transition: opacity 120ms ease;
}

.plugin-row:hover .plugin-actions,
.plugin-row:focus-within .plugin-actions,
.plugin-actions-attention {
  opacity: 1;
}

@media (pointer: coarse), (prefers-reduced-motion: reduce) {
  .plugin-actions {
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .plugin-actions {
    transition: none;
  }
}

@media (max-width: 40rem) {
  .plugin-actions {
    width: 100%;
    min-width: 0;
    justify-content: flex-start;
  }
}
</style>

<template>
  <article class="plugin-row py-3 border-b border-line stack gap-2 min-w-0" :data-plugin="plugin.pluginId">
    <div class="plugin-row-main row flex-wrap gap-3 min-w-0 items-start">
      <button type="button" class="synra-choice text-ink p-0 text-start focusable border-0 rounded-control bg-transparent flex-1 min-w-0" @click="$emit('details')">
        <span class="text-base font-medium block truncate">{{ plugin.title }}</span>
        <span class="text-sm text-ink-dim block truncate">{{ messages.plugins.states[plugin.state as keyof typeof messages.plugins.states] ?? plugin.state }} · {{ summary }}</span>
      </button>
      <div class="plugin-actions" :class="{ 'plugin-actions-attention': plugin.state === 'awaiting_consent' || plugin.state === 'faulted' }">
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
    </div>
    <p v-if="plugin.update && !plugin.localSource" class="text-sm text-primary-ink m-0">
      {{ plugin.update.needsNewConsent ? messages.plugins.updateNeedsConsent : messages.plugins.updateAvailable }}
    </p>
    <p v-if="attention" class="text-sm text-warn m-0">
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
const summary = computed(() => props.plugin.stateReason || (props.plugin.health && props.plugin.health.health !== 'good'
  ? messages.plugins.healthLabel(props.plugin.health.health)
  : props.plugin.description || props.plugin.pluginId))
</script>
