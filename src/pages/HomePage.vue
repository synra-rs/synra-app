<template>
  <section class="stack gap-8">
    <h1 class="text-xl font-semibold">
      {{ messages.home.title }}
    </h1>

    <ErrorNotice v-if="error" :code="error.code" :message="error.message" :actions="error.actions" @retry="load" />

    <!--
      ★ The desktop's own share entry. Everything else on this page is about intents that arrived from somewhere else;
      without this box the app could show what it received and never let the user start one.
    -->
    <div class="row gap-2">
      <AInput
        v-model:value="shareText"
        :placeholder="messages.home.share.placeholder"
        allow-clear
        @press-enter="share"
      />
      <AButton :loading="sharing" :disabled="shareText.trim() === ''" @click="share">
        {{ messages.home.share.action }}
      </AButton>
      <AButton :loading="sharingClipboard" @click="shareClipboard">
        {{ messages.home.share.clipboard }}
      </AButton>
    </div>

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
              <div class="min-w-0 stack gap-0.5">
                <span class="truncate text-sm">{{ transfer.peerName }}</span>
                <!-- A file's name is what a user recognises; a text send has none, and an empty line is better than "text". -->
                <span v-if="transfer.fileName" class="truncate text-xs text-ink-dim">{{ transfer.fileName }}</span>
              </div>
              <div class="row gap-2">
                <!--
                  The direction is a word, not a colour: the same row is used for both, and "receiving" is the fact that
                  decides which side the cancel command reaches (the listener's identifier or the sender's worker).
                -->
                <ATag class="m-0">
                  {{ directionOf(transfer) }}
                </ATag>
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
          <div class="stack gap-2">
            <div class="row gap-3">
              <ATag class="m-0" :color="intentStateColor(intent.state)">
                {{ intentStateLabel(intent.state) }}
              </ATag>
              <span v-if="intent.pluginId" class="truncate text-xs text-ink-dim font-mono">{{ intent.pluginId }}</span>
              <!-- ★ The id a bug report quotes: `synra logs --trace <id>` follows this one action across steps and devices. -->
              <span v-if="intent.traceId" class="truncate text-xs text-ink-dim font-mono" :title="intent.traceId">
                {{ messages.home.received.trace }} {{ intent.traceId }}
              </span>
            </div>

            <!--
              ★ "Running" is true for two different situations: the first attempt is still waiting, or this device has
              already sent it again several times. The kernel knows which; the card says it instead of leaving the user
              with a word that never changes.
            -->
            <p v-if="intent.retry" class="m-0 text-xs text-ink-dim">
              {{ retryLabel(intent.retry) }}
            </p>

            <!--
              ★ A candidate is a decision, not a status line. The kernel says which devices could run it; the page's job
              is to let the user pick one, and declining is the other half of that decision.
            -->
            <div v-for="candidate in intent.candidates" :key="candidate.deviceId" class="row-between gap-2">
              <span class="min-w-0 truncate text-xs">
                {{ candidate.title }} · {{ deviceName(candidate.deviceId) }}
                <span class="text-ink-dim font-mono">{{ candidate.deviceId }}</span>
                <span v-if="!candidate.online" class="text-ink-dim"> · {{ messages.home.received.offline }}</span>
              </span>
              <AButton
                size="small"
                :disabled="!candidate.online"
                :loading="deciding === intent.intentId"
                @click="accept(intent, candidate)"
              >
                {{ messages.home.received.accept }}
              </AButton>
            </div>

            <div v-if="canDecline(intent)" class="row gap-2">
              <AButton
                size="small"
                danger
                :loading="deciding === intent.intentId"
                @click="decline(intent)"
              >
                {{ messages.home.received.decline }}
              </AButton>
            </div>

            <!-- ★ The kernel's error is a code plus a message **key**; the sentence is this side's (R34). -->
            <p v-if="intent.error" class="m-0 text-xs">
              <span class="text-error">{{ intentError(intent) }}</span>
            </p>
          </div>
        </AList.Item>
      </AList>
    </section>

    <!--
      ★ The history the in-flight list cannot be. A transfer's row is removed when it finishes, and the file stays in the
      inbox; this is where a person finds what arrived earlier without being told to open a directory by hand.
    -->
    <section class="stack gap-3">
      <h2 class="text-base font-medium">
        {{ messages.home.receivedFiles.title }}
      </h2>

      <AEmpty v-if="received.length === 0" :description="messages.home.receivedFiles.empty" />

      <AList v-else size="small">
        <AList.Item v-for="file in received" :key="file.path">
          <div class="row-between gap-3">
            <div class="min-w-0 stack gap-0.5">
              <span class="truncate text-sm">{{ file.fileName }}</span>
              <span class="truncate text-xs text-ink-dim font-mono">{{ file.path }}</span>
            </div>
            <div class="row shrink-0 gap-2">
              <span class="text-xs text-ink-dim">{{ formatBytes(file.bytes) }}</span>
              <AButton size="small" :loading="revealing === file.path" @click="revealReceived(file)">
                {{ messages.home.receivedFiles.showInFolder }}
              </AButton>
              <AButton size="small" :loading="opening === file.path" @click="openReceived(file)">
                {{ messages.home.receivedFiles.open }}
              </AButton>
              <APopconfirm
                :title="messages.home.receivedFiles.deleteConfirm"
                :ok-text="messages.home.receivedFiles.delete"
                :cancel-text="messages.home.receivedFiles.keep"
                @confirm="discardReceived(file)"
              >
                <AButton size="small" danger :loading="discarding === file.path">
                  {{ messages.home.receivedFiles.delete }}
                </AButton>
              </APopconfirm>
            </div>
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
import type { IntentCandidate, IntentRetry, IntentSnapshot, ReceivedFile, SuggestedAction, TransferSnapshot } from '@synra/protocol'
import { Button as AButton, Empty as AEmpty, Input as AInput, List as AList, Popconfirm as APopconfirm, Progress as AProgress, Tag as ATag } from 'ant-design-vue'
import { storeToRefs } from 'pinia'
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import ErrorNotice from '../components/ErrorNotice.vue'

import { formatBytes } from '../format'
import { kernel } from '../kernel'
import { formatTime } from '../locale-format'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'
import { useIntentsStore } from '../stores/intents'
import { useNoticesStore } from '../stores/notices'
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

/** The text the user is sharing from this window. */
const shareText = ref('')

/** Whether the dispatch is in flight. */
const sharing = ref(false)

/** Whether the clipboard dispatch is in flight. */
const sharingClipboard = ref(false)

/**
 * Starts an intent from the text in the box.
 *
 * The source is the contract's `share` variant, not a URL-only shortcut: the same text may become a link, a note or a
 * plugin action depending on what the user installed, and deciding that here would be the page routing around the board.
 */
async function share(): Promise<void> {
  const text = shareText.value.trim()
  if (text === '')
    return
  error.value = undefined
  sharing.value = true
  const answer = await kernel.call<string>('intent_dispatch', {
    source: { kind: 'share', text, title: null }
  })
  sharing.value = false
  if (!answer.ok) {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  shareText.value = ''
  await load()
}

/**
 * Shares whatever is on the clipboard.
 *
 * The shell reads it exactly once, for this request, and refuses an empty clipboard. The page never reads it: a page
 * that could would be able to look at the clipboard whenever it liked.
 */
async function shareClipboard(): Promise<void> {
  error.value = undefined
  sharingClipboard.value = true
  const answer = await kernel.call<string>('intent_dispatch', {
    source: { kind: 'clipboard' }
  })
  sharingClipboard.value = false
  if (!answer.ok) {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  await load()
}

/**
 * The files already in the inbox, newest first.
 *
 * Read once at mount, and re-read only when a `notice.received` arrives: a page that polled would be the polling this
 * boundary exists to avoid, while a page that never re-read would show the inbox as it was when the window opened.
 */
const received = ref<readonly ReceivedFile[]>([])

/** Reads only the inbox list, for the refresh a notice triggers. */
async function refreshReceived(): Promise<void> {
  const answer = await kernel.call<readonly ReceivedFile[]>('transfer_received')
  if (answer.ok)
    received.value = answer.value
  else
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

// ★ A file that arrives while this page is open is a `notice`; the notice banner appears wherever the user is, and this
// is what keeps the list behind it from being a picture of the inbox at mount time. Only a received notice re-reads: the
// store is shared with other kinds, and a re-read for one of those would be a query nobody asked for.
const notices = useNoticesStore()
watch(() => notices.latest, (latest) => {
  if (latest?.message === 'notice.received')
    void refreshReceived()
})

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

/** The direction as a word, because it decides which side of the link the cancel command reaches. */
function directionOf(transfer: TransferSnapshot): string {
  return transfer.direction === 'receive' ? messages.home.inFlight.receiving : messages.home.inFlight.sending
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

  // ★ Three lists, because three panels are on this page. `intent_list` was called alone while `transfer_list` had no
  // transport behind it; that is no longer true — the shell has a listener and a transfer registry — so a transfer that
  // started before this window opened was invisible here until an event moved it. `transfer_received` is the history the
  // in-flight list cannot be: a finished row is removed, and the file stays in the inbox.
  const [intentAnswer, transferAnswer, receivedAnswer] = await Promise.all([
    kernel.call<readonly IntentSnapshot[]>('intent_list'),
    kernel.call<readonly TransferSnapshot[]>('transfer_list'),
    kernel.call<readonly ReceivedFile[]>('transfer_received')
  ])

  if (!intentAnswer.ok) {
    error.value = { code: intentAnswer.error.code, message: intentAnswer.error.message, actions: intentAnswer.error.actions }
    return
  }
  if (!transferAnswer.ok) {
    error.value = { code: transferAnswer.error.code, message: transferAnswer.error.message, actions: transferAnswer.error.actions }
    return
  }
  if (!receivedAnswer.ok) {
    error.value = { code: receivedAnswer.error.code, message: receivedAnswer.error.message, actions: receivedAnswer.error.actions }
    return
  }
  intentsStore.applySnapshot({ intents: intentAnswer.value, transfers: transferAnswer.value })
  received.value = receivedAnswer.value
}

/** The received file whose Open command is in flight. */
const opening = ref<string | undefined>(undefined)

/** The intent whose decision is in flight, so its buttons can say so and cannot be pressed twice. */
const deciding = ref<string | undefined>(undefined)

/** Sends a decision for one intent and reloads the board's picture. */
async function decide(intent: IntentSnapshot, decision: Record<string, unknown>): Promise<void> {
  error.value = undefined
  deciding.value = intent.intentId
  const answer = await kernel.call<null>('intent_decide', {
    intentId: intent.intentId,
    decision
  })
  deciding.value = undefined
  if (!answer.ok) {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  await load()
}

/** Runs an intent on the device the user picked. */
async function accept(intent: IntentSnapshot, candidate: IntentCandidate): Promise<void> {
  await decide(intent, { decision: 'accept', deviceId: candidate.deviceId })
}

/** Refuses an intent. The other side is told, which is why the reason is `null` rather than omitted. */
async function decline(intent: IntentSnapshot): Promise<void> {
  await decide(intent, { decision: 'decline', reason: null })
}

/**
 * Whether the intent is still the user's to refuse.
 *
 * A finished intent has nothing to decline, and a `Decline` button on one would offer an action the kernel refuses. The
 * board's final states are `IntentState::final_kinds` as the interface spells them, and `noHandler` is one of them.
 */
function canDecline(intent: IntentSnapshot): boolean {
  return !['succeeded', 'failed', 'noHandler', 'timedOut', 'refused', 'cancelled'].includes(intent.state)
}

/** The name a person gave the device, when the device list knows it; the id is still shown beside it. */
function deviceName(deviceId: string): string {
  return devices.byId.get(deviceId)?.displayName ?? deviceId
}

/** The board's state as a person's word; an unknown state is shown as the engine spelled it rather than blank. */
function intentStateLabel(state: string): string {
  return (messages.home.received.states as Record<string, string | undefined>)[state] ?? state
}

/** The tag's colour. Colour alone is not the state — the label says the same thing — but it is what makes a list scannable. */
function intentStateColor(state: string): string {
  if (state === 'succeeded')
    return 'success'
  if (state === 'failed' || state === 'timedOut' || state === 'refused')
    return 'error'
  if (state === 'noHandler')
    return 'warning'
  if (state === 'dispatching')
    return 'processing'
  return 'default'
}

/**
 * How a dispatch's retries read.
 *
 * ★ The two sentences are different on purpose: one is work still being attempted, the other is this process having
 * stopped. Neither changes the state word — the intent is still dispatching — so the words must not imply it failed.
 */
function retryLabel(retry: IntentRetry): string {
  if (retry.gaveUp)
    return messages.home.received.noAnswer
  const template = retry.nextAtMs === undefined
    ? messages.home.received.retrying
    : messages.home.received.retryingAt
  return template
    .replace('{attempts}', String(retry.attempts))
    .replace('{max}', String(retry.maxAttempts))
    .replace('{time}', retry.nextAtMs === undefined ? '' : formatClock(retry.nextAtMs))
}

/**
 * The clock time of the next attempt.
 *
 * ★ An absolute time rather than "in 30s": the card only re-renders when the kernel says something changed, and a
 * countdown baked into a render would be wrong a second later while this stays true until the next retry.
 */
function formatClock(at: number): string {
  return formatTime(at, true)
}

/**
 * Why an intent failed, in this side's words.
 *
 * The same lookup `ErrorNotice` does, and for the same reason: `IpcError.message` is a key and the code is finer than a
 * sentence. An unknown pair still says something true rather than printing either the key or the code.
 */
function intentError(intent: IntentSnapshot): string {
  const error = intent.error
  if (!error)
    return ''
  return messages.kernelErrors[error.message as keyof typeof messages.kernelErrors]
    ?? messages.errors[error.code as keyof typeof messages.errors]
    ?? messages.errors.unknown
}

/** The received file whose Show-in-folder command is in flight. */
const revealing = ref<string | undefined>(undefined)

/** The received file whose delete is in flight. */
const discarding = ref<string | undefined>(undefined)

/**
 * Deletes a received file, after the row's confirmation.
 *
 * The list is updated only when the shell says the file is gone: removing the row first would show a file as deleted
 * while the command could still refuse. The shell runs the same inbox containment check as Open and Reveal.
 */
async function discardReceived(file: ReceivedFile): Promise<void> {
  error.value = undefined
  discarding.value = file.path
  const answer = await kernel.call<null>('transfer_discard', { path: file.path })
  discarding.value = undefined
  if (!answer.ok) {
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
    return
  }
  received.value = received.value.filter(candidate => candidate.path !== file.path)
}

/**
 * Shows a received file in the platform's file manager.
 *
 * The shell runs the same inbox containment check as Open: revealing a path tells the user what is on their disk, and a
 * webview that could name any path would turn "where did that file land" into a file browser.
 */
async function revealReceived(file: ReceivedFile): Promise<void> {
  error.value = undefined
  revealing.value = file.path
  const answer = await kernel.call<null>('transfer_reveal', { path: file.path })
  revealing.value = undefined
  if (!answer.ok)
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

/**
 * Opens a received file with the platform's default application.
 *
 * The shell checks the path is inside the inbox before launching anything; this side only carries the one the list
 * returned. A refusal is shown where every other one on this page is.
 */
async function openReceived(file: ReceivedFile): Promise<void> {
  error.value = undefined
  opening.value = file.path
  const answer = await kernel.call<null>('transfer_open', { path: file.path })
  opening.value = undefined
  if (!answer.ok)
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

onMounted(load)
</script>
