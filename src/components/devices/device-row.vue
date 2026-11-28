<style scoped>
.device-actions {
  min-width: 10rem;
}

.device-details-action {
  opacity: 0;
  transition: opacity 120ms ease;
}

article:hover .device-details-action,
article:focus-within .device-details-action,
.device-details-action[aria-expanded="true"] {
  opacity: 1;
}

@media (pointer: coarse), (prefers-reduced-motion: reduce) {
  .device-details-action {
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .device-details-action {
    transition: none;
  }
}

@media (max-width: 40rem) {
  .device-actions {
    width: 100%;
    min-width: 0;
    justify-content: flex-start;
  }
}
</style>

<template>
  <article class="py-3 border-b border-line min-w-0">
    <div class="row-between flex-wrap gap-3 min-w-0">
      <div class="stack flex-1 gap-1 min-w-0">
        <h3 class="text-base font-medium m-0 truncate" :title="card.displayName">
          {{ card.displayName }}
        </h3>
        <div class="text-sm text-ink-dim row flex-wrap gap-x-3 gap-y-1">
          <span>{{ card.platform }}</span>
          <span>{{ card.online ? messages.devices.online : messages.devices.offline }}</span>
          <ATag v-if="card.blocked" color="warning">
            {{ messages.devices.blocked }}
          </ATag>
        </div>
      </div>
      <!--
        ★ `row-between` already puts this block at the row's right edge, so `ms-auto` here would do nothing. What does
          not line up is the block's own right edge: it is as wide as its contents, and a paired device has no Pair
          button, so the two rows' Details buttons settled at different x while both were "right aligned".
      -->
      <div class="device-actions row flex-wrap gap-2 items-center justify-end">
        <AButton v-if="!card.paired && !card.blocked" type="primary" :aria-label="messages.devices.pairingWith(card.displayName)" @click="emit('pair', card)">
          {{ messages.devices.pairing }}
        </AButton>
        <AButton class="device-details-action" type="text" :aria-label="messages.devices.detailsFor(card.displayName)" :aria-expanded="expanded" :aria-controls="detailsId" @click="expanded = !expanded">
          {{ messages.devices.details }}
          <span :class="expanded ? 'i-carbon-chevron-up' : 'i-carbon-chevron-down'" aria-hidden="true" />
        </AButton>
      </div>
    </div>
    <div v-if="expanded" :id="detailsId" class="mt-4 stack gap-4 min-w-0" role="region" :aria-label="messages.devices.detailsFor(card.displayName)">
      <dl class="text-base m-0 gap-x-6 gap-y-3 grid sm:grid-cols-[auto_minmax(0,1fr)]">
        <dt class="text-ink-dim">
          {{ messages.devices.name }}
        </dt>
        <dd class="m-0 min-w-0 break-words">
          {{ card.displayName }}
        </dd>
        <dt class="text-ink-dim">
          {{ messages.devices.deviceId }}
        </dt>
        <dd class="font-mono m-0 break-all">
          {{ card.deviceId }}
        </dd>
        <template v-if="card.address">
          <dt class="text-ink-dim">
            {{ messages.devices.address }}
          </dt>
          <dd class="font-mono m-0 break-all">
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
      <div v-if="card.paired || card.blocked">
        <AButton danger @click="emit('forget', card)">
          {{ card.blocked ? messages.devices.unblock : messages.devices.forget }}
        </AButton>
      </div>
    </div>
  </article>
</template>

<script setup lang="ts">
import type { DeviceCard } from '@synra/protocol'
import { Button as AButton, Tag as ATag } from 'ant-design-vue'
import { ref, useId } from 'vue'
import { messages } from '../../messages'
import { formatDateTime } from '../../utils/locale-format'

defineProps<{ card: DeviceCard }>()
const emit = defineEmits<{ forget: [card: DeviceCard], pair: [card: DeviceCard] }>()
const expanded = ref(false)
const detailsId = useId()
</script>
