<template>
  <section class="stack gap-4" :aria-label="messages.plugins.workflow.permissions">
    <h3 class="m-0 text-base font-medium">
      {{ messages.plugins.workflow.permissions }}
    </h3>
    <p v-if="!capabilities.length" class="m-0 text-base text-ink-dim">
      {{ messages.plugins.preview.none }}
    </p>
    <div v-for="capability in capabilities" :key="capability" class="row-between items-start gap-3 border-b border-line py-3">
      <span class="min-w-0 stack gap-1">
        <span class="text-base">{{ capabilityPurpose(capability) }}</span>
        <code class="break-all text-sm text-ink-dim">{{ capability }}</code>
      </span>
      <span class="shrink-0 text-sm text-ink-dim">{{ plugin.granted.includes(capability) ? messages.plugins.workflow.allowed : messages.plugins.workflow.denied }}</span>
    </div>
  </section>
</template>

<script setup lang="ts">
import type { PluginSnapshot } from '@synra/protocol'
import { computed } from 'vue'
import { capabilityPurpose } from '../../features/plugins/capabilities'
import { messages } from '../../messages'

const props = defineProps<{ plugin: PluginSnapshot }>()
const capabilities = computed(() => [...new Set(props.plugin.requested ?? [])])
</script>
