<template>
  <header class="min-w-0 row-between flex-wrap gap-3 border-b border-line pb-4">
    <h1 class="m-0 min-w-0 text-xl font-semibold leading-tight">
      {{ title }}
    </h1>
    <div v-if="$slots.default" class="min-w-0 row flex-wrap gap-2 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8">
      <slot />
    </div>
  </header>
</template>

<script setup lang="ts">
defineProps<{ title: string }>()
</script>
