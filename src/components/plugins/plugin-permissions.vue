<template>
  <section class="stack gap-4" :aria-label="messages.plugins.workflow.permissions">
    <h3 class="text-base font-medium m-0">
      {{ messages.plugins.workflow.permissions }}
    </h3>
    <p v-if="!capabilities.length" class="text-base text-ink-dim m-0">
      {{ messages.plugins.preview.none }}
    </p>
    <div v-for="capability in capabilities" :key="capability" class="py-3 border-b border-line row-between gap-3 items-start">
      <span class="stack gap-1 min-w-0">
        <span class="text-base">{{ capabilityPurpose(capability) }}</span>
        <code class="text-sm text-ink-dim break-all">{{ capability }}</code>
      </span>
      <span class="text-sm text-ink-dim shrink-0">{{ plugin.granted.includes(capability) ? messages.plugins.workflow.allowed : messages.plugins.workflow.denied }}</span>
    </div>
  </section>
</template>

<script setup lang="ts">
import type { PluginSnapshot } from '@synra/protocol'
import { computed } from 'vue'
import { capabilityPurpose } from '../../features/plugins/capabilities'
import { messages } from '../../messages'

const props = defineProps<{ plugin: PluginSnapshot }>()
const capabilities = computed(() => [
  ...new Set(props.plugin.requested ?? [])
])
</script>
