<template>
  <!--
    A reading page: status, what needs doing, and recent activity. [03 §3](../../design-docs/ui-design/03-shell-and-navigation.md)
    gives single-column reading/form pages 640–768px; `max-w-3xl` is the 768px end, and the shell's 1200px guard rail
    would otherwise let these short rows stretch across a wide window with nothing to fill the middle.
  -->
  <section class="mx-auto max-w-3xl min-w-0 stack gap-6 pb-4 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8" data-page-width="reading">
    <PageHeader :title="messages.home.title">
      <AButton @click="router.push('/devices')">
        {{ wire.state === 'none' ? messages.home.wire.action : messages.home.inFlight.action }}
      </AButton>
    </PageHeader>

    <ErrorNotice v-if="error" :code="error.code" :actions="error.actions" @retry="load" />

    <!-- A compact connection surface; glass is reserved for actual overlays. -->
    <section class="min-w-0 stack gap-3 border border-line rounded-surface bg-surface p-4" :aria-label="messages.home.wire.label">
      <div class="row-between flex-wrap gap-2">
        <span class="text-xs text-ink-dim">{{ messages.home.wire.label }}</span>
        <span class="text-sm font-medium">{{ state.headline }}</span>
      </div>
      <div class="min-w-0 row gap-3">
        <span class="max-w-1/3 min-w-0 truncate text-sm" :title="devices.identity?.displayName">
          {{ devices.identity?.displayName ?? messages.app.name }}
        </span>
        <span class="relative h-0.5 min-w-6 flex-1 rounded-pill bg-line" aria-hidden="true">
          <span class="absolute start-0 inset-y-0 rounded-pill bg-primary" :class="[wireWidth, wireLive && 'shadow-glow']" />
        </span>
        <span class="max-w-1/3 min-w-0 truncate text-end text-sm" :class="wirePeerName ? 'text-ink' : 'text-ink-dim'" :title="wirePeerName">
          {{ wirePeerName ?? messages.home.wire.action }}
        </span>
      </div>
      <!--
        ★ `'detail' in state`, not `wire.state === 'none' || 'offline'`: only those two states define a detail, and
        asking for the property is the check TypeScript can actually narrow on. Writing the state names again here
        was a second copy of the same fact — it just disagreed with the message file whenever one changed.
      -->
      <p v-if="'detail' in state" class="m-0 text-xs text-ink-dim">
        {{ state.detail }}
      </p>
    </section>

    <section class="stack gap-3">
      <h2 class="m-0 text-sm font-medium">
        {{ messages.home.inFlight.title }}
      </h2>

      <p v-if="transfers.length === 0" class="m-0 border-b border-line py-4 text-sm text-ink-dim">
        {{ messages.home.inFlight.empty }}
      </p>

      <AList v-else size="small">
        <AList.Item v-for="transfer in transfers" :key="transfer.transferId" :style="{ paddingInline: 0 }">
          <div class="min-w-0 w-full stack gap-2">
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
                  {{ stateLabel(transfer) }}
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
                  overlay-class-name="[&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-8"
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
      <h2 class="m-0 text-sm font-medium">
        {{ messages.home.received.title }}
      </h2>

      <p v-if="intents.length === 0" class="m-0 border-b border-line py-4 text-sm text-ink-dim">
        {{ messages.home.received.empty }}
      </p>

      <AList v-else size="small">
        <AList.Item v-for="intent in intents" :key="intent.intentId" :style="{ paddingInline: 0 }">
          <div class="min-w-0 w-full stack gap-2">
            <div class="min-w-0 row flex-wrap gap-3">
              <ATag class="m-0" :color="intentStateColor(intent.state)">
                {{ intentStateLabel(intent.state) }}
              </ATag>
              <span v-if="intent.pluginId" class="max-w-full min-w-0 break-all text-xs text-ink-dim font-mono">{{ intent.pluginId }}</span>
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
                <!--
                ★ The id keeps a `title` even though the row truncates it: `deviceName` falls back to this id when the
                device is not in the store, so on an unregistered device the clipped id is the *only* text in the row
                that names it — and two devices with the same display name differ only here.
              -->
                <span class="text-ink-dim font-mono" :title="candidate.deviceId">{{ candidate.deviceId }}</span>
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

            <!--
              ★ The row carries what a person scanning the list needs; this holds what they need once they have decided
              to look. [05 §5](../design-docs/05-home-and-activity.md) requires the trace to be **readable and copyable in
              full** — a disclosure rather than a permanent line, because a 26-character id on every row is noise until
              someone is actually chasing one action.
            -->
            <div v-if="intent.traceId || intent.startedAtMs !== undefined" class="min-w-0 stack gap-1">
              <!--
                ★ `min-h-11`, not `min-h-8`: `09 §3` sets 44px as the floor for anything a pointer or a thumb has to hit,
                and this is the only toggle on the page that was under it. It sits inside a row whose height it no longer
                matches, so it stays `self-start`.
              -->
              <button
                type="button"
                class="min-h-11 self-start border-0 rounded bg-transparent px-0 text-xs text-ink-dim focusable md:min-h-8 hover:text-ink"
                :aria-expanded="expanded === intent.intentId"
                @click="toggleExpanded(intent.intentId)"
              >
                {{ messages.home.received.details }}
              </button>
              <div v-if="expanded === intent.intentId" class="stack gap-1 text-xs">
                <div v-if="intent.startedAtMs !== undefined" class="row flex-wrap items-baseline gap-2">
                  <span class="text-ink-dim">{{ messages.home.received.startedAt }}</span>
                  <span class="min-w-0 break-all font-mono">{{ formatClock(intent.startedAtMs) }}</span>
                </div>
                <div v-if="intent.traceId" class="row flex-wrap items-center gap-2">
                  <span class="text-ink-dim">{{ messages.home.received.trace }}</span>
                  <!-- The id alone, so a triple-click or the button both yield something pasteable into a command. -->
                  <span class="min-w-0 select-text break-all font-mono" :data-testid="`trace-${intent.intentId}`">{{ intent.traceId }}</span>
                  <AButton size="small" :data-testid="`copy-${intent.intentId}`" @click="copyTrace(intent.intentId, intent.traceId)">
                    {{ copiedTrace === intent.traceId
                      ? messages.home.received.copied
                      : selectedTrace === intent.intentId
                        ? messages.home.received.selectInstead
                        : messages.home.received.copy }}
                  </AButton>
                </div>
              </div>
            </div>
          </div>
        </AList.Item>
      </AList>
    </section>

    <!--
      ★ The history the in-flight list cannot be. A transfer's row is removed when it finishes, and the file stays in the
      inbox; this is where a person finds what arrived earlier without being told to open a directory by hand.
    -->
    <section class="stack gap-3">
      <h2 class="m-0 text-sm font-medium">
        {{ messages.home.receivedFiles.title }}
      </h2>

      <p v-if="received.length === 0" class="m-0 py-4 text-sm text-ink-dim">
        {{ messages.home.receivedFiles.empty }}
      </p>

      <AList v-else size="small">
        <AList.Item v-for="file in received" :key="file.path" :style="{ paddingInline: 0 }">
          <div class="row-between gap-3">
            <div class="min-w-0 stack gap-0.5">
              <span class="truncate text-sm" :title="file.fileName">{{ file.fileName }}</span>
              <!--
                ★ Not truncated: [05 §3](../design-docs/05-home-and-activity.md) makes the full path part of what this row
                owes, and a path is the one thing a person needs in order to open the file. Cutting it off left long
                paths with nothing to fall back on — there is no detail view here to open. `title` is the shortcut for
                the common case; the wrapping line is the answer.
              -->
              <span class="break-all text-xs text-ink-dim font-mono" :title="file.path">{{ file.path }}</span>
              <!-- ★ `receivedAtMs` is the write's end. [05 §5](../design-docs/05-home-and-activity.md) draws the line
                   between a start and a finish, so the label says received rather than started. -->
              <span class="text-xs text-ink-dim" :title="formatDateTime(file.receivedAtMs)">
                {{ messages.home.receivedFiles.receivedAt }} {{ formatDateTime(file.receivedAtMs) }}
              </span>
            </div>
            <span class="shrink-0 text-xs text-ink-dim">{{ formatBytes(file.bytes) }}</span>
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
import { Button as AButton, List as AList, Popconfirm as APopconfirm, Progress as AProgress, Tag as ATag } from 'ant-design-vue'
import { storeToRefs } from 'pinia'
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import ErrorNotice from '../components/ErrorNotice.vue'
import PageHeader from '../components/PageHeader.vue'

import { formatBytes } from '../format'
import { kernel } from '../kernel'
import { formatDateTime, formatTime } from '../locale-format'
import { messages } from '../messages'
import { errorSentence } from '../messages/errors'
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

/**
 * ★ `state` is a wire name (`active`, `offered`), not a word. The row used to print it raw, so a Chinese screen showed
 * "active" — and a reader who did not know the protocol could not tell a moving transfer from one waiting to be accepted.
 * An unrecognised state keeps its own name rather than borrowing a nearby one. The `keyof` cast and the `??` fallback are
 * the same pair `PluginsPage` uses for `plugin.state`, so both screens answer "unknown" the same way.
 */
function stateLabel(transfer: TransferSnapshot): string {
  return messages.home.inFlight.states[transfer.state as keyof typeof messages.home.inFlight.states] ?? transfer.state
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
  const transfer = transfers.value[0]
  if (transfer)
    return { state: 'transferring' as const, device: devices.cards.find(card => card.deviceId === transfer.peerDevice) }

  const reachable = devices.cards.find(card => card.paired && card.online)
  if (reachable)
    return { state: 'connected' as const, device: reachable }

  const paired = devices.cards.find(card => card.paired)
  if (paired)
    return { state: 'offline' as const, device: paired }

  return { state: 'none' as const, device: undefined }
})

const state = computed(() => messages.home.wire.states[wire.value.state])
const wirePeerName = computed(() => wire.value.state === 'transferring'
  ? wire.value.device?.displayName ?? transfers.value[0]?.peerName
  : wire.value.device?.displayName)

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

/** The intent whose details are open; at most one, because two open disclosures in a list read as two claims. */
const expanded = ref<string | undefined>(undefined)
/** The trace just copied, so exactly one button says "Copied" and only until attention moves on. */
const copiedTrace = ref<string | undefined>(undefined)
/** The row whose clipboard write failed and whose id is selected instead, so its button says what to press. */
const selectedTrace = ref<string | undefined>(undefined)

function toggleExpanded(intentId: string): void {
  expanded.value = expanded.value === intentId ? undefined : intentId
}

/**
 * Copies the trace id and nothing else.
 *
 * [05 §5](../design-docs/05-home-and-activity.md): the copy is the value, not the label — a person pastes this into
 * `synra logs --trace`, and "Trace 01HQ…" would be a command that fails.
 *
 * ★ When the clipboard refuses (no user activation, a denied permission, an insecure context) the id is selected
 * instead of the click doing nothing. A button that reports neither success nor failure is one the user presses
 * twice and then types the id by hand without knowing they had to.
 */
async function copyTrace(intentId: string, traceId: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(traceId)
  }
  catch {
    selectTrace(intentId)
    return
  }
  copiedTrace.value = traceId
  window.setTimeout(() => {
    if (copiedTrace.value === traceId)
      copiedTrace.value = undefined
  }, 2000)
}

/** Marks the attempt as a selection so the button says what to do next, and selects the text it names. */
function selectTrace(intentId: string): void {
  selectedTrace.value = intentId
  const node = document.querySelector(`[data-testid="trace-${CSS.escape(intentId)}"]`)
  if (!node)
    return
  const range = document.createRange()
  range.selectNodeContents(node)
  const selection = window.getSelection()
  selection?.removeAllRanges()
  selection?.addRange(range)
  window.setTimeout(() => {
    if (selectedTrace.value === intentId)
      selectedTrace.value = undefined
  }, 4000)
}

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
  // ★ Every state between "a share arrived" and "something ran" is progress the reader is waiting on. Only
  // `dispatching` had a colour, so `matching`, `choosing` and `confirming` wore the same grey as `cancelled` — the
  // one state here that means nothing is going to happen.
  if (state === 'matching' || state === 'choosing' || state === 'confirming' || state === 'dispatching')
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
  return errorSentence(error.code)
}

onMounted(load)
</script>
