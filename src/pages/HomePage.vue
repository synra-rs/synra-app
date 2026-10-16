<template>
  <section class="stack gap-8">
    <h1 class="text-xl font-semibold">
      {{ messages.home.title }}
    </h1>

    <ErrorNotice v-if="error" :code="error.code" :message="error.message" :actions="error.actions" @retry="load" />

    <!--
      ★ The wire (R27). This is the one thing on this screen worth spending boldness on, and the only place the frosted
      material and the glow are allowed to mean something: `floating` is the glass (R43), and `shadow-glow` appears only
      when the line is *live* (R45 — glow is a state, not a skin).

      It shows a real state, never a decorative animation (R27), and the state is written out as well as drawn, because
      colour alone is not a state (R23).
    -->
    <div class="flex flex-col gap-4 floating p-5">
      <p class="text-xs text-ink-dim">
        {{ messages.home.wire.label }}
      </p>

      <div class="row gap-3 sm:gap-4">
        <span class="shrink-0 truncate text-sm font-medium">
          {{ devices.identity?.displayName ?? messages.app.name }}
        </span>

        <span class="relative h-0.5 min-w-6 flex-1 rounded-pill bg-line" aria-hidden="true">
          <span
            class="absolute start-0 inset-y-0 rounded-pill bg-primary"
            :class="[wireWidth, wireLive && 'shadow-glow']"
          />
        </span>

        <span class="shrink-0 truncate text-sm" :class="wire.device ? 'text-ink' : 'text-ink-dim'">
          {{ wire.device?.displayName ?? messages.devices.empty }}
        </span>
      </div>

      <div class="stack">
        <p class="text-base font-medium">
          {{ state.headline }}
        </p>
        <p class="text-sm text-ink-dim">
          {{ state.detail }}
        </p>
      </div>

      <!-- R28: an empty state is an invitation with a next step, not a sentence. -->
      <RouterLink
        v-if="wire.state === 'none'"
        to="/devices"
        class="w-fit row gap-2 border border-line-strong rounded-control px-3 py-2 text-sm text-primary-ink focusable @hover:bg-raised"
      >
        {{ messages.home.wire.action }}
        <span class="i-carbon-arrow-right" aria-hidden="true" />
      </RouterLink>
    </div>

    <section class="stack gap-3">
      <h2 class="text-base font-medium">
        {{ messages.home.inFlight.title }}
      </h2>

      <p v-if="transfers.length === 0" class="row gap-3 text-sm text-ink-dim">
        {{ messages.home.inFlight.empty }}
        <RouterLink to="/devices" class="text-primary-ink underline underline-offset-2 focusable">
          {{ messages.home.inFlight.action }}
        </RouterLink>
      </p>

      <ul v-else class="m-0 stack list-none gap-0 p-0">
        <li
          v-for="transfer in transfers"
          :key="transfer.transferId"
          class="row-between gap-3 border-t border-line py-3 first:border-t-0"
        >
          <span class="truncate text-sm">{{ transfer.peerName }}</span>
          <span class="shrink-0 text-xs text-ink-dim">{{ transfer.state }}</span>
          <span class="shrink-0 text-xs text-ink-dim font-mono">{{ transfer.kind }}</span>
        </li>
      </ul>
    </section>

    <section class="stack gap-3">
      <h2 class="text-base font-medium">
        {{ messages.home.received.title }}
      </h2>

      <p v-if="intents.length === 0" class="row gap-3 text-sm text-ink-dim">
        {{ messages.home.received.empty }}
        <RouterLink to="/devices" class="text-primary-ink underline underline-offset-2 focusable">
          {{ messages.home.received.action }}
        </RouterLink>
      </p>

      <ul v-else class="m-0 stack list-none gap-0 p-0">
        <li
          v-for="intent in intents"
          :key="intent.intentId"
          class="row gap-3 border-t border-line py-3 first:border-t-0"
        >
          <span class="rounded-pill bg-raised px-2 py-0.5 text-xs text-ink-dim">{{ intent.state }}</span>
          <span v-if="intent.pluginId" class="truncate text-xs text-ink-dim font-mono">{{ intent.pluginId }}</span>
        </li>
      </ul>
    </section>
  </section>
</template>

<script setup lang="ts">
/**
 * What this device is doing, when nothing in particular needs doing.
 *
 * # Why this page exists rather than a dashboard
 *
 * A home page that aggregates everything is a page whose every number is a copy of a number somewhere else, and every
 * copy is stale from the moment it is written. So this one answers a single question — *is anything on its way to
 * another device* — and points at the pages that answer the rest.
 *
 * # The wire is derived, not animated (R27)
 *
 * Its state comes from the same snapshot the rest of the interface reads: a send in flight, a device that is both paired
 * and reachable, a paired device that is not answering, or nothing paired at all. Against the fake kernel the last one is
 * what is drawn — honestly, with the action that changes it (R28) — rather than a decorative loop that would look alive
 * while saying nothing.
 */
import type { IntentSnapshot, SuggestedAction, TransferSnapshot } from '@synra/protocol'
import { computed, onMounted, ref } from 'vue'

import ErrorNotice from '../components/ErrorNotice.vue'
import { kernel } from '../kernel'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'

const devices = useDevicesStore()

const intents = ref<readonly IntentSnapshot[]>([])
const transfers = ref<readonly TransferSnapshot[]>([])
/** The refusal itself, not its sentence: the surface needs the `code`, the message **key** and the `actions` (R33). */
const error = ref<{ code: string, message: string, actions?: readonly SuggestedAction[] | undefined } | undefined>(undefined)

/** Which of the four real states the wire is in, and which device it is about. */
const wire = computed(() => {
  if (transfers.value.length > 0)
    return { state: 'transferring' as const, device: devices.cards.find(card => card.paired && card.online) }

  const reachable = devices.cards.find(card => card.paired && card.online)
  if (reachable)
    return { state: 'connected' as const, device: reachable }

  const paired = devices.cards.find(card => card.paired)
  if (paired)
    return { state: 'offline' as const, device: paired }

  return { state: 'none' as const, device: undefined }
})

const state = computed(() => messages.home.wire.states[wire.value.state])

/**
 * The four states are distinct as **shapes**, not only as colours (R23): nothing paired is a short stub, a paired device
 * is a line that reaches across, and a live line also glows. A greyscale screenshot still tells them apart.
 */
const wireWidth = computed(() => {
  switch (wire.value.state) {
    case 'transferring': return 'w-1/2'
    case 'connected': return 'w-full'
    case 'offline': return 'w-full opacity-40'
    default: return 'w-1/12'
  }
})

/** R45: the glow means "this is live", so it is on for exactly the two live states. */
const wireLive = computed(() => wire.value.state === 'connected' || wire.value.state === 'transferring')

async function load(): Promise<void> {
  error.value = undefined

  const asked = await Promise.all([
    kernel.call<{ intents: readonly IntentSnapshot[] }>('intent_snapshot'),
    kernel.call<{ transfers: readonly TransferSnapshot[] }>('transfer_list')
  ])

  const [intentAnswer, transferAnswer] = asked
  if (intentAnswer?.ok)
    intents.value = intentAnswer.value.intents
  if (transferAnswer?.ok)
    transfers.value = transferAnswer.value.transfers

  // Both refusals are reported rather than swallowed: a panel that renders "nothing on its way" when the kernel could
  // not be asked is worse than an empty one, because it is an answer.
  const refusal = asked.find(answer => answer && !answer.ok)
  if (refusal && !refusal.ok)
    error.value = { code: refusal.error.code, message: refusal.error.message, actions: refusal.error.actions }
}

onMounted(load)
</script>
