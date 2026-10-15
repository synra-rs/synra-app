<style scoped>
h1 {
  margin: 0 0 20px;
  font-size: 22px;
}

.list {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--panel);
}

.card__head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.state {
  padding: 2px 10px;
  border-radius: 999px;
  background: rgb(255 255 255 / 6%);
  font-size: 12px;
  color: var(--muted);
}

.state--active {
  color: var(--accent);
}

.state--faulted {
  color: var(--bad);
}

.desc {
  margin: 0;
  font-size: 13px;
}

.caps {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.caps__label {
  min-width: 64px;
  color: var(--muted);
  font-size: 12px;
}

.cap {
  padding: 1px 7px;
  border-radius: 5px;
  background: rgb(91 157 255 / 12%);
  color: var(--accent);
  font-size: 12px;
}

.cap--bad {
  background: rgb(224 108 117 / 12%);
  color: var(--bad);
}

.update {
  margin: 0;
  padding-top: 8px;
  border-top: 1px solid var(--border);
  color: var(--warn);
  font-size: 13px;
}

.muted {
  color: var(--muted);
}

.bad {
  color: var(--bad);
}
</style>

<template>
  <section>
    <h1>Plugins</h1>

    <p v-if="error" class="bad">
      {{ error }}
    </p>
    <p v-else-if="plugins.length === 0" class="muted">
      No plugins are installed.
    </p>

    <ul class="list">
      <li v-for="plugin in plugins" :key="plugin.pluginId" class="card">
        <div class="card__head">
          <div>
            <strong>{{ plugin.title }}</strong>
            <span class="muted"> {{ plugin.pluginId }} · v{{ plugin.version }} · {{ plugin.publisher }}</span>
          </div>
          <span class="state" :class="`state--${plugin.state}`">{{ plugin.state }}</span>
        </div>

        <p v-if="plugin.description" class="muted desc">
          {{ plugin.description }}
        </p>

        <div class="caps">
          <span class="caps__label">granted</span>
          <span v-if="plugin.granted.length === 0" class="muted">nothing</span>
          <code v-for="capability in plugin.granted" :key="capability" class="cap">{{ capability }}</code>
        </div>

        <div class="caps">
          <span class="caps__label">refused</span>
          <span v-if="plugin.denied.length === 0" class="muted">nothing</span>
          <code v-for="capability in plugin.denied" :key="capability" class="cap cap--bad">{{ capability }}</code>
        </div>

        <p v-if="plugin.update" class="update">
          v{{ plugin.update.version }} is available
          <template v-if="plugin.update.needsNewConsent">
            — it wants new permission: {{ plugin.update.addedCapabilities.join(', ') }}
          </template>
        </p>
      </li>
    </ul>
  </section>
</template>

<script setup lang="ts">
/**
 * What is installed, and what each one has been allowed to do.
 *
 * # Why granted and denied are shown rather than only granted
 *
 * A permissions screen that lists what a plugin *may* do tells the user nothing about what it *has tried* to do. The
 * design document asks for both, and it is the denied column that turns the screen from a settings page into the thing a
 * user actually reads before installing something.
 *
 * # Why the lifecycle state is a field and not a colour
 *
 * The second plugin in the fake snapshot is `enabled` **and** `faulted`. A screen that derived its badge from `enabled`
 * would show that plugin as working, which is the specific mistake `PluginSnapshot.state` exists to prevent.
 */
import type { PluginSnapshot } from '@synra/protocol'
import { onMounted, ref } from 'vue'

import { kernel } from '../kernel'

const plugins = ref<readonly PluginSnapshot[]>([])
const error = ref('')

onMounted(async () => {
  const answer = await kernel.call<{ plugins: readonly PluginSnapshot[] }>('plugins_list')
  if (answer.ok)
    plugins.value = answer.value.plugins
  else
    error.value = answer.error.message
})
</script>
