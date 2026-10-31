<!--
  antd's collapse arrow, without the name it announces.

  `@ant-design/icons-vue` wraps every icon in `<span role="img" aria-label="right">` and only marks the inner `<svg>`
  `aria-hidden`. A `role="img"` label replaces its content in the name computation, so a panel header read as
  "right 性能" — an English direction word in front of a Chinese title. Measured on the plugins page with
  `bsk_inspect`'s `html`; the diagnostics page's five panels and the credential panel had the same fault.

  The arrow is decoration: the header already carries `aria-expanded`, so `aria-hidden` here loses nothing and stops
  the word being said twice.

  ★ One component rather than a slot on each `ACollapse`, because the slot has to sit on the *collapse*, not on its
    panels (`Collapse.js` builds the arrow once in `renderExpandIcon` from `props.expandIcon ?? slots.expandIcon` and
    passes it down) — a rule that is easy to get wrong in three places and impossible to notice when it is.

  ★ `isActive` is not in the slot's declared type, so it is read off the panel props. The runtime value is the
    panel's own, which is what `renderExpandIcon` passes.
-->
<template>
  <span
    :class="(panel as { isActive?: boolean }).isActive ? 'i-carbon-chevron-down' : 'i-carbon-chevron-right'"
    aria-hidden="true"
    class="shrink-0 text-ink-dim"
  />
</template>

<script setup lang="ts">
defineProps<{ panel: unknown }>()
</script>
