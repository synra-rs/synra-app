<style scoped>
h1 {
  margin: 0 0 20px;
  font-size: 22px;
}

h2 {
  margin: 24px 0 10px;
  font-size: 14px;
  color: var(--muted);
  text-transform: uppercase;
  letter-spacing: 0.06em;
}

.pairs {
  display: grid;
  grid-template-columns: 140px 1fr;
  gap: 8px 16px;
  margin: 0;
}

dt {
  color: var(--muted);
}

dd {
  margin: 0;
}

.note {
  margin: 0 0 12px;
  font-size: 13px;
  max-width: 60ch;
}

code {
  padding: 1px 6px;
  border-radius: 5px;
  background: rgb(255 255 255 / 6%);
  font-size: 12px;
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
    <h1>Settings</h1>

    <p v-if="error" class="bad">
      {{ error }}
    </p>

    <h2>This device</h2>
    <dl v-if="identity" class="pairs">
      <dt>Name</dt>
      <dd>
        {{ identity.displayName }}
        <span v-if="identity.nameIsDefault" class="muted">(generated — you can rename it)</span>
      </dd>
      <dt>Platform</dt>
      <dd>{{ identity.platform }}</dd>
      <dt>Kernel</dt>
      <dd>v{{ identity.kernelVersion }}</dd>
      <dt>Device id</dt>
      <dd><code>{{ identity.deviceId }}</code></dd>
    </dl>

    <h2>Preferences</h2>
    <p class="muted note">
      These read from the kernel. Writing them back is not wired in this milestone, so the controls are shown disabled
      rather than made to look like they work.
    </p>
    <dl class="pairs">
      <template v-for="key in KEYS" :key="key">
        <dt>{{ key }}</dt>
        <dd>
          <code>{{ JSON.stringify(config[key] ?? null) }}</code>
        </dd>
      </template>
    </dl>
  </section>
</template>

<script setup lang="ts">
/**
 * This device's own settings.
 *
 * # Why this page reads the kernel rather than `localStorage`
 *
 * A preference stored in the browser is a preference that does not follow the user to the other devices, does not appear
 * in a diagnostic bundle, and is not the thing `config_get` returns. The design document is explicit that configuration
 * is the kernel's, so this page round-trips a real command even though the value it changes is not written back yet.
 *
 * # Why nothing is written back in M0
 *
 * `config_set` exists and is not called from here. A toggle that appears to work and does not is worse than one that is
 * visibly absent: the user changes a setting, sees the shell's own state move, restarts the app, and is now debugging a
 * bug in the settings page. So the control is rendered disabled, with the reason.
 */
import type { IdentitySnapshot } from '@synra/protocol'
import { onMounted, ref } from 'vue'

import { kernel } from '../kernel'

/** The keys this page knows about, in the order they are shown. */
const KEYS = ['theme', 'autoSyncPlugins'] as const

const config = ref<Record<string, unknown>>({})
const identity = ref<IdentitySnapshot | undefined>(undefined)
const error = ref('')

onMounted(async () => {
  const [configAnswer, identityAnswer] = await Promise.all([
    kernel.call<Record<string, unknown>>('config_get'),
    kernel.call<IdentitySnapshot>('identity_get')
  ])

  if (configAnswer.ok)
    config.value = configAnswer.value
  else
    error.value = configAnswer.error.message

  if (identityAnswer.ok)
    identity.value = identityAnswer.value
})
</script>
