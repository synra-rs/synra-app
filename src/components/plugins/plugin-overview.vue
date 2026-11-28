<template>
  <div class="stack gap-4">
    <p v-if="plugin.description" class="text-base m-0">
      {{ plugin.description }}
    </p>
    <dl class="text-base m-0 gap-x-4 gap-y-3 grid grid-cols-[auto_minmax(0,1fr)]">
      <dt class="text-ink-dim">
        {{ messages.plugins.pluginId }}
      </dt><dd class="font-mono m-0 break-all">
        {{ plugin.pluginId }}
      </dd>
      <dt class="text-ink-dim">
        {{ messages.plugins.version }}
      </dt><dd class="font-mono m-0">
        {{ plugin.version }}
      </dd>
      <dt class="text-ink-dim">
        {{ messages.plugins.publisher }}
      </dt><dd class="m-0 break-all">
        {{ plugin.publisher }}
      </dd>
    </dl>
    <p v-if="plugin.stateReason" class="text-base text-warn m-0">
      {{ plugin.stateReason }}
    </p>
    <p v-if="plugin.health && plugin.health.health !== 'good'" class="text-base text-warn m-0">
      {{ messages.plugins.healthLabel(plugin.health.health) }}
    </p>
    <div v-if="plugin.update" class="p-3 border border-line rounded-control stack gap-2">
      <p class="text-base m-0">
        {{ messages.plugins.updateFound.replace('{version}', plugin.update.version) }}
      </p>
      <p class="text-sm text-ink-dim m-0">
        {{ plugin.update.needsNewConsent ? messages.plugins.updateNeedsConsent : messages.plugins.updateNoNewPermissions }}
      </p>
      <code v-for="capability in plugin.update.addedCapabilities" :key="capability" class="text-sm break-all">{{ capability }}</code>
    </div>
    <p v-if="plugin.updateCheckedAtMs !== undefined" class="text-sm text-ink-dim m-0">
      {{ messages.plugins.lastChecked.replace('{time}', formatDateTime(plugin.updateCheckedAtMs)) }}
    </p>
    <details v-if="quotaRows(plugin.health).length" class="text-sm">
      <summary class="synra-choice text-ink-dim min-h-8 cursor-pointer">
        {{ messages.plugins.allLimits }}
      </summary>
      <dl class="m-0 pt-2 gap-2 grid grid-cols-[auto_minmax(0,1fr)]">
        <template v-for="row in quotaRows(plugin.health)" :key="row.label">
          <dt>{{ row.label }}</dt><dd class="m-0">
            {{ row.value }}
          </dd>
        </template>
      </dl>
    </details>
  </div>
</template>

<script setup lang="ts">
import type { PluginSnapshot } from '@synra/protocol'
import { quotaRows } from '../../features/plugins/quota'
import { messages } from '../../messages'
import { formatDateTime } from '../../utils/locale-format'

defineProps<{ plugin: PluginSnapshot }>()
</script>
