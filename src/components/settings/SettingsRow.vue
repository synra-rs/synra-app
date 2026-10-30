<template>
  <div class="grid min-w-0 gap-3 py-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,22rem)] sm:items-start sm:gap-x-6 first:pt-0">
    <div class="min-w-0 stack gap-1">
      <label v-if="forId" :for="forId" class="text-sm font-medium">{{ label }}</label>
      <span v-else class="text-sm font-medium">{{ label }}</span>
      <p v-if="description" class="m-0 text-xs text-ink-dim leading-relaxed">
        {{ description }}
      </p>
    </div>
    <div class="min-w-0 stack items-start gap-2 [&_.ant-btn]:min-h-11 [&_.ant-select]:max-w-full sm:items-end md:[&_.ant-btn]:min-h-8">
      <slot />
    </div>
    <div v-if="$slots.feedback" class="min-w-0 sm:col-span-2">
      <slot name="feedback" />
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{ label: string, description?: string | undefined, forId?: string | undefined }>()
</script>
