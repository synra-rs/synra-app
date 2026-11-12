<template>
  <ADropdown :trigger="['click']">
    <AButton :type="compact ? 'text' : 'default'" :class="{ 'synra-icon-button': compact }" :disabled="disabled" :aria-label="messages.plugins.workflow.more">
      <span v-if="!compact">{{ messages.plugins.workflow.more }}</span>
      <span :class="compact ? 'i-carbon-overflow-menu-horizontal' : 'i-carbon-chevron-down'" aria-hidden="true" />
    </AButton>
    <template #overlay>
      <AMenu @click="info => emit('action', info.key as PluginMenuAction)">
        <template v-if="compact">
          <AMenuItem key="overview">
            {{ messages.plugins.workflow.details }}
          </AMenuItem>
          <AMenuItem key="permissions">
            {{ messages.plugins.workflow.permissions }}
          </AMenuItem>
          <AMenuItem key="audit">
            {{ messages.plugins.audit.title }}
          </AMenuItem>
          <AMenuDivider />
        </template>
        <AMenuItem :key="plugin.localSource ? 'reload' : 'check'">
          {{ plugin.localSource ? messages.plugins.workflow.reloadAction : messages.plugins.checkUpdates }}
        </AMenuItem>
        <AMenuItem v-if="plugin.update && !plugin.localSource" key="update">
          {{ messages.plugins.updateAction }}
        </AMenuItem>
        <AMenuDivider />
        <AMenuItem key="sync">
          {{ messages.plugins.syncAction }}
        </AMenuItem>
        <AMenuItem v-if="!compact" key="audit">
          {{ messages.plugins.audit.title }}
        </AMenuItem>
        <AMenuDivider />
        <AMenuItem key="remove" danger>
          {{ messages.plugins.removeAction }}
        </AMenuItem>
      </AMenu>
    </template>
  </ADropdown>
</template>

<script setup lang="ts">
import type { PluginSnapshot } from '@synra/protocol'
import type { PluginMenuAction } from '../../features/plugins/menu'
import { Button as AButton, Dropdown as ADropdown, Menu as AMenu, MenuDivider as AMenuDivider, MenuItem as AMenuItem } from 'ant-design-vue'
import { messages } from '../../messages'

defineProps<{ plugin: PluginSnapshot, disabled?: boolean, compact?: boolean }>()
const emit = defineEmits<{ action: [action: PluginMenuAction] }>()
</script>
