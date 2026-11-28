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
  <header class="page-header pb-4 border-b border-line row-between flex-wrap gap-3 min-w-0">
    <h1 class="text-lg leading-tight font-semibold m-0 min-w-0">
      {{ title }}
    </h1>
    <div v-if="$slots.default" class="page-header-actions row flex-wrap gap-2 min-w-0">
      <slot />
    </div>
  </header>
</template>

<script setup lang="ts">
defineProps<{ title: string }>()
</script>
