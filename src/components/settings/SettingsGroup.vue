<template>
  <section class="min-w-0 stack gap-3">
    <h3 class="m-0 text-sm text-ink-dim font-medium">
      {{ title }}
    </h3>
    <div class="min-w-0 stack gap-0 divide-y divide-line">
      <slot />
    </div>
  </section>
</template>

<script setup lang="ts">
defineProps<{ title: string }>()
</script>
