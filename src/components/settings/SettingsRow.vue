<style scoped>
.settings-feedback-slot {
  min-height: 1.25rem;
}
</style>

<template>
  <div class="synra-choice py-4 gap-3 grid min-w-0 first:pt-0 sm:gap-x-6 sm:grid-cols-[minmax(0,1fr)_minmax(0,22rem)] sm:items-start">
    <div class="stack gap-1 min-w-0">
      <label v-if="forId" :for="forId" class="text-base font-medium">{{ label }}</label>
      <span v-else class="text-base font-medium">{{ label }}</span>
      <p v-if="description" class="text-sm text-ink-dim leading-relaxed m-0">
        {{ description }}
      </p>
    </div>
    <div class="gap-2[&_.ant-select]:max-w-full stack min-w-0 items-start sm:items-end">
      <slot />
    </div>
    <div v-if="$slots.feedback" class="settings-feedback-slot min-w-0 sm:col-span-2">
      <slot name="feedback" />
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{ label: string, description?: string | undefined, forId?: string | undefined }>()
</script>
