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

      It is also the **one** piece of the interface that is deliberately not the component library's. antd has no such
      control, and the whole point of the rule is that a screen has one memorable thing — wrapping it in a Card would
      make it furniture. Everything around it is antd: the actions, the lists, the tags.

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
      <AButton v-if="wire.state === 'none'" class="w-fit" @click="router.push('/devices')">
        {{ messages.home.wire.action }}
      </AButton>
    </div>

    <section class="stack gap-3">
      <h2 class="text-base font-medium">
        {{ messages.home.inFlight.title }}
      </h2>

      <AEmpty v-if="transfers.length === 0" :description="messages.home.inFlight.empty">
        <AButton @click="router.push('/devices')">
          {{ messages.home.inFlight.action }}
        </AButton>
      </AEmpty>

      <AList v-else size="small">
        <AList.Item v-for="transfer in transfers" :key="transfer.transferId">
          <div class="stack gap-2">
            <div class="row-between gap-3">
              <span class="truncate text-sm">{{ transfer.peerName }}</span>
              <div class="row gap-2">
                <ATag class="m-0">
                  {{ transfer.state }}
                </ATag>
                <!--
                  The same command for both directions: an outgoing transfer stops its worker, an incoming one leaves the
                  identifier for the listener, which is the only writer to that link.
                  ★ Asked first: a cancel throws away bytes that may be most of a file, and a button a slip of the mouse
                  can press is a button that will be pressed by mistake.
                -->
                <APopconfirm
                  v-if="askBeforeCancel"
                  :title="messages.home.inFlight.confirm"
                  :ok-text="messages.home.inFlight.abandon"
                  :cancel-text="messages.home.inFlight.keep"
                  @confirm="cancelTransfer(transfer)"
                >
                  <AButton size="small" :loading="cancelling === transfer.transferId">
                    {{ messages.home.inFlight.cancel }}
                  </AButton>
                </APopconfirm>
                <AButton
                  v-else
                  size="small"
                  :loading="cancelling === transfer.transferId"
                  @click="cancelTransfer(transfer)"
                >
                  {{ messages.home.inFlight.cancel }}
                </AButton>
              </div>
            </div>
            <!--
              ★ The bytes, not a spinner: "transferring" with no number is a state a user cannot judge, and a bar with no
              figure is one they cannot quote in a bug report.
            -->
            <div class="row gap-3">
              <AProgress
                class="flex-1"
                :percent="percentOf(transfer)"
                size="small"
              />
              <span class="text-xs text-ink-dim">{{ progressLabel(transfer) }}</span>
            </div>
          </div>
        </AList.Item>
      </AList>
    </section>

    <section class="stack gap-3">
      <h2 class="text-base font-medium">
        {{ messages.home.received.title }}
      </h2>

      <AEmpty v-if="intents.length === 0" :description="messages.home.received.empty">
        <AButton @click="router.push('/devices')">
          {{ messages.home.received.action }}
        </AButton>
      </AEmpty>

      <AList v-else size="small">
        <AList.Item v-for="intent in intents" :key="intent.intentId">
          <div class="row gap-3">
            <ATag class="m-0">
              {{ intent.state }}
            </ATag>
            <span v-if="intent.pluginId" class="truncate text-xs text-ink-dim font-mono">{{ intent.pluginId }}</span>
          </div>
        </AList.Item>
      </AList>
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
 *
 * # What is antd here, and what is not
 *
 * The buttons, the lists, the tags and the error surface are `ant-design-vue`, so they carry the theme the rest of the
 * application does (R51). The wire is not: it is the screen's one designed object, and the component library does not
 * have one. That is the split the design asks for — the library owns the controls, the interface owns the one thing
 * worth looking at.
 */
import type { IntentSnapshot, SuggestedAction, TransferSnapshot } from '@synra/protocol'
import { Button as AButton, Empty as AEmpty, List as AList, Popconfirm as APopconfirm, Progress as AProgress, Tag as ATag } from 'ant-design-vue'
import { storeToRefs } from 'pinia'
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'

import ErrorNotice from '../components/ErrorNotice.vue'
import { formatBytes } from '../format'
import { kernel } from '../kernel'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'
import { useIntentsStore } from '../stores/intents'
import { useSettingsStore } from '../stores/settings'

const router = useRouter()
const devices = useDevicesStore()
const settings = useSettingsStore()

/** Whether an in-flight transfer asks before it is abandoned; absent means yes. */
const askBeforeCancel = computed(() => settings.value('transfers.confirmCancel') !== false)

// ★ The intent store, not two local refs. The kernel publishes `intentUpdated` / `transferUpdated` and `src/events.ts`
// applies them to this store; a page holding its own copy would show the list it read at mount while the store moved on —
// and the two would disagree about exactly the thing the user is waiting to see.
const intentsStore = useIntentsStore()
const { intents, transfers } = storeToRefs(intentsStore)

/** The refusal itself, not its sentence: the surface needs the `code`, the message **key** and the `actions` (R33). */
const error = ref<{ code: string, message: string, actions?: readonly SuggestedAction[] | undefined } | undefined>(undefined)

/** The transfer being cancelled, so its button can say so and cannot be pressed twice. */
const cancelling = ref<string | undefined>(undefined)

/** How far the transfer has got, clamped to what a bar can draw. */
function percentOf(transfer: TransferSnapshot): number {
  if (transfer.totalBytes <= 0)
    return 0
  return Math.min(100, Math.round((transfer.receivedBytes / transfer.totalBytes) * 100))
}

/** "12.3 KiB of 3.0 MiB", so the bar can be quoted as well as seen. */
function progressLabel(transfer: TransferSnapshot): string {
  return messages.home.inFlight.progress
    .replace('{done}', formatBytes(transfer.receivedBytes))
    .replace('{total}', formatBytes(transfer.totalBytes))
}

/** Asks the kernel to abandon a transfer, whichever direction it is moving. */
async function cancelTransfer(transfer: TransferSnapshot): Promise<void> {
  cancelling.value = transfer.transferId
  error.value = undefined
  const answer = await kernel.call<null>('transfer_cancel', { transferId: transfer.transferId })
  cancelling.value = undefined
  if (!answer.ok)
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

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

  // ★ `intent_list` only. `transfer_list` was called here beside it, and this shell does not register it — there is no
  // transport, so there is nothing to list. The call could therefore only fail, and it did so into a branch that ignored
  // the failure: the panel showed "nothing on its way" whether the answer was "none" or "you cannot ask". An empty list
  // that is known to be empty is the honest shape; the command returns when the transport does.
  const answer = await kernel.call<readonly IntentSnapshot[]>('intent_list')

  if (answer.ok) {
    intents.value = answer.value
    transfers.value = []
    return
  }

  // The refusal is reported rather than swallowed: a panel that renders "nothing on its way" when the kernel could not be
  // asked is worse than an empty one, because it is an answer.
  error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

onMounted(load)
</script>
