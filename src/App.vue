<style scoped>
.shell {
  display: grid;
  grid-template-columns: 240px 1fr;
  min-height: 100vh;
}

.side {
  display: flex;
  flex-direction: column;
  gap: 20px;
  padding: 20px 16px;
  border-right: 1px solid var(--border);
  background: var(--panel);
}

.brand {
  display: flex;
  gap: 8px;
  align-items: center;
  font-weight: 600;
  font-size: 16px;
}

.mark {
  color: var(--accent);
}

nav {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.link {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  border-radius: 8px;
  color: var(--text);
  text-decoration: none;
}

.link:hover {
  background: rgb(255 255 255 / 4%);
}

.link--on {
  background: rgb(91 157 255 / 12%);
}

.link__label {
  font-weight: 500;
}

.link__hint,
.self span {
  color: var(--muted);
  font-size: 12px;
}

.self {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding-top: 12px;
  border-top: 1px solid var(--border);
  font-size: 12px;
}

.main {
  padding: 28px 32px;
  max-width: 900px;
}

.muted {
  color: var(--muted);
}
</style>

<template>
  <div class="shell">
    <aside class="side">
      <div class="brand">
        <span class="mark">◆</span>
        <span>Synra</span>
      </div>

      <nav>
        <RouterLink
          v-for="page in PAGES"
          :key="page.path"
          :to="page.path"
          class="link"
          active-class="link--on"
          exact-active-class="link--on"
        >
          <span class="link__label">{{ page.label }}</span>
          <span class="link__hint">{{ page.hint }}</span>
        </RouterLink>
      </nav>

      <div class="self">
        <template v-if="devices.identity">
          <strong>{{ devices.identity.displayName }}</strong>
          <span>{{ devices.identity.platform }} · v{{ devices.identity.kernelVersion }}</span>
        </template>
        <template v-else>
          <span class="muted">loading…</span>
        </template>
      </div>
    </aside>

    <main class="main">
      <RouterView />
    </main>
  </div>
</template>

<script setup lang="ts">
/**
 * The frame every page renders inside: identity on the left, the page on the right.
 *
 * # Why the identity is in the shell and not on each page
 *
 * "Which device am I looking at" is answered the same way everywhere, so answering it in four places is four chances to
 * answer it differently. The kernel's identity snapshot is fetched once, here, and the pages that need it read the store.
 */
import type { DevicesSnapshot } from '@synra/protocol'
import { onMounted } from 'vue'

import { kernel } from './kernel'
import { PAGES } from './router'
import { useDevicesStore } from './stores/devices'

const devices = useDevicesStore()

onMounted(async () => {
  // The snapshot is taken before anything subscribes, which is the wrong order in general and the right one *here*:
  // the interface is starting up with nothing on screen, so there is no event it can miss yet. Once a page is live the
  // dispatcher owns the ordering.
  const answer = await kernel.call<DevicesSnapshot>('devices_snapshot')
  if (answer.ok)
    devices.applySnapshot(answer.value)
})
</script>
