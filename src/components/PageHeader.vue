<style scoped>
.page-header {
  min-height: 2.5rem;
  align-items: flex-start;
}

.page-header-actions {
  min-height: 2rem;
  min-width: max-content;
  justify-content: flex-end;
}

@media (max-width: 40rem) {
  .page-header-actions {
    width: 100%;
    min-width: 0;
  }
}
</style>

<template>
  <header class="page-header min-w-0 row-between flex-wrap gap-3 border-b border-line pb-4">
    <h1 class="m-0 min-w-0 text-lg font-semibold leading-tight">
      {{ title }}
    </h1>
    <div v-if="$slots.default" class="page-header-actions min-w-0 row flex-wrap gap-2">
      <slot />
    </div>
  </header>
</template>

<script setup lang="ts">
defineProps<{ title: string }>()
</script>
