<template>
  <section class="stack gap-3 min-w-0">
    <h3 class="text-base text-ink-dim font-medium m-0">
      {{ title }}
    </h3>
    <div class="stack gap-0 min-w-0 divide-line divide-y">
      <slot />
    </div>
  </section>
</template>

<script setup lang="ts">
defineProps<{ title: string }>()
</script>
