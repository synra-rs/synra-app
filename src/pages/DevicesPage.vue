<style scoped>
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

h1 {
  margin: 0 0 20px;
  font-size: 22px;
}

.btn {
  margin-bottom: 20px;
  padding: 7px 14px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--panel);
  color: var(--text);
  cursor: pointer;
}

.btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.scan {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 16px;
}

.scan__bar {
  height: 4px;
  border-radius: 999px;
  background: rgb(255 255 255 / 8%);
  overflow: hidden;
}

.scan__fill {
  height: 100%;
  background: var(--accent);
  transition: width 0.2s ease;
}

.list {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 16px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--panel);
}

.card__main {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.card__tags {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.pill {
  padding: 2px 9px;
  border-radius: 999px;
  background: rgb(255 255 255 / 6%);
  font-size: 12px;
  color: var(--muted);
}

.pill--warn {
  color: var(--warn);
}

.pill--bad {
  color: var(--bad);
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
    <div class="head">
      <h1>Devices</h1>
      <button class="btn" :disabled="devices.scanning" @click="scan">
        {{ devices.scanning ? 'Scanning…' : 'Scan' }}
      </button>
    </div>

    <p v-if="error" class="bad">
      {{ error }}
    </p>

    <div v-if="devices.scan" class="scan">
      <div class="scan__bar">
        <div
          class="scan__fill"
          :style="{ width: `${Math.min(100, (devices.scan.elapsedMs / devices.scan.totalMs) * 100).toFixed(0)}%` }"
        />
      </div>
      <span class="muted">
        found {{ devices.scan.found }} · {{ devices.scan.elapsedMs }}ms of {{ devices.scan.totalMs }}ms
      </span>
    </div>

    <p v-if="devices.cards.length === 0" class="muted">
      No devices yet. Start a scan.
    </p>

    <ul class="list">
      <li v-for="card in devices.cards" :key="card.deviceId" class="card">
        <div class="card__main">
          <strong>{{ card.displayName }}</strong>
          <span class="muted">{{ card.platform }}</span>
        </div>
        <div class="card__tags">
          <span class="pill" :class="{ 'pill--warn': !card.paired }">
            {{ card.paired ? 'paired' : 'not paired' }}
          </span>
          <span class="pill" :class="{ 'pill--bad': !card.online }">
            {{ card.online ? 'online' : 'offline' }}
          </span>
          <span v-if="card.address" class="pill">{{ card.address }}</span>
        </div>
      </li>
    </ul>
  </section>
</template>

<script setup lang="ts">
/**
 * Pairing, sending, and what is nearby.
 *
 * # Why the scan is worth a button here
 *
 * The snapshot is a *picture*, and a picture of a list of devices does not show that the list is live. The scan is the
 * one action whose whole effect arrives as events, so it is the only way to exercise the path the design document cares
 * about — listen first, then apply by sequence number, and re-read on a gap.
 *
 * Against the fake kernel that path is real: `devices_scan_start` makes the fake publish `scanUpdated` and then
 * `deviceCardUpdated`, and the dispatcher's `SequenceTracker` judges every one of them.
 *
 * # Why paired and online are two columns
 *
 * They answer different questions. A paired device that is offline is one the user can still send to later; a device that
 * is online but unpaired is one the user has to pair first. Collapsing them into a status string loses exactly the case
 * that needs the user to act.
 */
import type { DevicesSnapshot } from '@synra/protocol'
import { onMounted, onUnmounted, ref } from 'vue'

import { KERNEL_EVENT_CHANNEL } from '../ipc/fake-kernel'
import { kernel } from '../kernel'
import { useDevicesStore } from '../stores/devices'

const devices = useDevicesStore()
const error = ref('')
let stop: (() => void) | undefined

onMounted(async () => {
  const answer = await kernel.call<DevicesSnapshot>('devices_snapshot')
  if (answer.ok)
    devices.applySnapshot(answer.value)
  else
    error.value = answer.error.message

  // The listener is registered after the snapshot above, which is the order the client documents as the caller's to get
  // right. On startup nothing is on screen, so there is nothing to miss; the scan below is where the ordering earns
  // its keep.
  const unsubscribe = await kernel.subscribe(KERNEL_EVENT_CHANNEL, () => {
    // The payload's `type` is read here only to decide whether this store cares; the dispatcher does the real work of
    // deciding whether it is *safe* to apply.
  })
  stop = unsubscribe
})

onUnmounted(() => {
  stop?.()
})

async function scan(): Promise<void> {
  error.value = ''
  const answer = await kernel.call('devices_scan_start')
  if (!answer.ok)
    error.value = answer.error.message
}
</script>
