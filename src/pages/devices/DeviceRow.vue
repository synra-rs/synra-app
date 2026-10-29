<template>
  <article class="min-w-0 border-b border-line py-4 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8">
    <div class="min-w-0 row-between flex-wrap gap-3">
      <div class="min-w-0 stack flex-1 gap-1">
        <h3 class="m-0 truncate text-sm font-medium" :title="card.displayName">
          {{ card.displayName }}
        </h3>
        <div class="row flex-wrap gap-x-3 gap-y-1 text-xs text-ink-dim">
          <span>{{ card.platform }}</span>
          <span>{{ card.online ? messages.devices.online : messages.devices.offline }}</span>
          <ATag v-if="card.blocked" color="warning">
            {{ messages.devices.blocked }}
          </ATag>
        </div>
      </div>
      <div class="row flex-wrap gap-2">
        <AButton v-if="!card.paired" :disabled="pairingBusy" @click="emit('pair', card.deviceId)">
          {{ messages.devices.pair }}
        </AButton>
        <AButton type="text" :aria-label="messages.devices.detailsFor(card.displayName)" :aria-expanded="expanded" :aria-controls="detailsId" @click="expanded = !expanded">
          {{ messages.devices.details }}
          <span :class="expanded ? 'i-carbon-chevron-up' : 'i-carbon-chevron-down'" aria-hidden="true" />
        </AButton>
      </div>
    </div>
    <div v-if="expanded" :id="detailsId" class="mt-4 min-w-0 stack gap-4" role="region" :aria-label="messages.devices.detailsFor(card.displayName)">
      <dl class="grid m-0 gap-x-6 gap-y-3 text-sm sm:grid-cols-[auto_minmax(0,1fr)]">
        <dt class="text-ink-dim">
          {{ messages.devices.name }}
        </dt>
        <dd class="m-0 min-w-0 break-words">
          {{ card.displayName }}
        </dd>
        <dt class="text-ink-dim">
          {{ messages.devices.deviceId }}
        </dt>
        <dd class="m-0 break-all font-mono">
          {{ card.deviceId }}
        </dd>
        <template v-if="card.address">
          <dt class="text-ink-dim">
            {{ messages.devices.address }}
          </dt>
          <dd class="m-0 break-all font-mono">
            {{ card.address }}
          </dd>
        </template>
        <dt class="text-ink-dim">
          {{ messages.devices.lastSeen }}
        </dt>
        <dd class="m-0">
          <time v-if="card.lastSeenMs > 0" :datetime="new Date(card.lastSeenMs).toISOString()">{{ formatDateTime(card.lastSeenMs) }}</time>
          <span v-else>{{ messages.devices.unknownTime }}</span>
        </dd>
      </dl>
      <div v-if="card.paired">
        <AButton danger @click="emit('forget', card)">
          {{ messages.devices.forget }}
        </AButton>
      </div>
    </div>
  </article>
</template>

<script setup lang="ts">
import type { DeviceCard } from '@synra/protocol'
import { Button as AButton, Tag as ATag } from 'ant-design-vue'
import { ref, useId } from 'vue'
import { formatDateTime } from '../../locale-format'
import { messages } from '../../messages'

defineProps<{ card: DeviceCard, pairingBusy: boolean }>()
const emit = defineEmits<{ pair: [deviceId: string], forget: [card: DeviceCard] }>()
const expanded = ref(false)
const detailsId = useId()
</script>
