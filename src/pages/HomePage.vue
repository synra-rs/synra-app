<style scoped>
h1 {
  margin: 0 0 20px;
  font-size: 22px;
}

h2 {
  margin: 28px 0 10px;
  font-size: 14px;
  color: var(--muted);
  text-transform: uppercase;
  letter-spacing: 0.06em;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 12px;
}

.tile {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 16px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--panel);
  color: var(--text);
  text-decoration: none;
}

.tile:hover {
  border-color: var(--accent);
}

.tile span {
  color: var(--muted);
  font-size: 12px;
}

.list {
  margin: 0;
  padding-left: 18px;
}

.muted {
  color: var(--muted);
}

.bad {
  color: var(--bad);
}

.pill {
  display: inline-block;
  padding: 1px 8px;
  border-radius: 999px;
  background: rgb(255 255 255 / 6%);
  font-size: 12px;
}
</style>

<template>
  <section>
    <h1>Home</h1>

    <p v-if="failed" class="bad">
      The kernel refused the snapshot: <code>{{ failed }}</code>
    </p>

    <div class="grid">
      <RouterLink to="/devices" class="tile">
        <strong>Devices</strong>
        <span>Pair a device, send it something, see what is nearby.</span>
      </RouterLink>
      <RouterLink to="/plugins" class="tile">
        <strong>Plugins</strong>
        <span>Install one, and see exactly what it has been allowed to do.</span>
      </RouterLink>
      <RouterLink to="/settings" class="tile">
        <strong>Settings</strong>
        <span>This device's name, its platform, and its own preferences.</span>
      </RouterLink>
    </div>

    <h2>In flight</h2>
    <p v-if="transfers.length === 0" class="muted">
      Nothing is being transferred.
    </p>
    <ul v-else class="list">
      <li v-for="transfer in transfers" :key="transfer.transferId">
        {{ transfer.kind }} → {{ transfer.peerName }} · {{ transfer.state }}
      </li>
    </ul>

    <h2>Recent intents</h2>
    <p v-if="intents.length === 0" class="muted">
      Nothing has been shared to this device yet.
    </p>
    <ul v-else class="list">
      <li v-for="intent in intents" :key="intent.intentId">
        <span class="pill">{{ intent.state }}</span>
        <span v-if="intent.pluginId"> {{ intent.pluginId }}</span>
      </li>
    </ul>
  </section>
</template>

<script setup lang="ts">
/**
 * What this device is doing, when nothing in particular needs doing.
 *
 * # Why this page exists rather than a dashboard
 *
 * A home page that aggregates everything is a page whose every number is a copy of a number somewhere else, and every
 * copy is stale from the moment it is written. So this one answers a single question — *is anything in flight* — and
 * links to the pages that answer the rest.
 *
 * The fake kernel has no transfers to show, so the empty state is rendered for real rather than hidden. A panel whose
 * empty state was never built is a panel whose empty state is discovered by a user.
 */
import type { IntentSnapshot, TransferSnapshot } from '@synra/protocol'
import { onMounted, ref } from 'vue'

import { kernel } from '../kernel'

const intents = ref<readonly IntentSnapshot[]>([])
const transfers = ref<readonly TransferSnapshot[]>([])
const failed = ref<string>('')

onMounted(async () => {
  const asked = await Promise.all([
    kernel.call<{ intents: readonly IntentSnapshot[] }>('intent_snapshot'),
    kernel.call<{ transfers: readonly TransferSnapshot[] }>('transfer_list')
  ])

  const [intentAnswer, transferAnswer] = asked
  if (intentAnswer?.ok)
    intents.value = intentAnswer.value.intents
  if (transferAnswer?.ok)
    transfers.value = transferAnswer.value.transfers

  // Both refusals are reported rather than swallowed: a panel that renders "nothing in flight" when the kernel could
  // not be asked is worse than an empty one, because it is an answer.
  const refusal = [intentAnswer, transferAnswer].find(answer => answer && !answer.ok)
  if (refusal && !refusal.ok)
    failed.value = refusal.error.message
})
</script>
