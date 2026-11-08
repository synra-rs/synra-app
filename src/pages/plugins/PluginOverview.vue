<template>
  <div class="stack gap-4">
    <p v-if="plugin.description" class="m-0 text-base">
      {{ plugin.description }}
    </p>
    <dl class="grid grid-cols-[auto_minmax(0,1fr)] m-0 gap-x-4 gap-y-3 text-base">
      <dt class="text-ink-dim">
        {{ messages.plugins.pluginId }}
      </dt><dd class="m-0 break-all font-mono">
        {{ plugin.pluginId }}
      </dd>
      <dt class="text-ink-dim">
        {{ messages.plugins.version }}
      </dt><dd class="m-0 font-mono">
        {{ plugin.version }}
      </dd>
      <dt class="text-ink-dim">
        {{ messages.plugins.publisher }}
      </dt><dd class="m-0 break-all">
        {{ plugin.publisher }}
      </dd>
    </dl>
    <p v-if="plugin.stateReason" class="m-0 text-base text-warn">
      {{ plugin.stateReason }}
    </p>
    <p v-if="plugin.health && plugin.health.health !== 'good'" class="m-0 text-base text-warn">
      {{ messages.plugins.healthLabel(plugin.health.health) }}
    </p>
    <div v-if="plugin.update" class="stack gap-2 border border-line rounded-control p-3">
      <p class="m-0 text-base">
        {{ messages.plugins.updateFound.replace('{version}', plugin.update.version) }}
      </p>
      <p class="m-0 text-sm text-ink-dim">
        {{ plugin.update.needsNewConsent ? messages.plugins.updateNeedsConsent : messages.plugins.updateNoNewPermissions }}
      </p>
      <code v-for="capability in plugin.update.addedCapabilities" :key="capability" class="break-all text-sm">{{ capability }}</code>
    </div>
    <p v-if="plugin.updateCheckedAtMs !== undefined" class="m-0 text-sm text-ink-dim">
      {{ messages.plugins.lastChecked.replace('{time}', formatDateTime(plugin.updateCheckedAtMs)) }}
    </p>
    <details v-if="quotaRows(plugin.health).length" class="text-sm">
      <summary class="synra-choice min-h-8 cursor-pointer text-ink-dim">
        {{ messages.plugins.allLimits }}
      </summary>
      <dl class="grid grid-cols-[auto_minmax(0,1fr)] m-0 gap-2 pt-2">
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
import { messages } from '../../messages'
import { formatDateTime } from '../../utils/locale-format'
import { quotaRows } from './quota'

defineProps<{ plugin: PluginSnapshot }>()
</script>
