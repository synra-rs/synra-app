<template>
  <section class="stack gap-6">
    <div class="row-between gap-4">
      <h1 class="text-xl font-semibold">
        {{ messages.devices.title }}
      </h1>

      <!--
        The scan is a control, so it is the component library's (R51): "you cannot press it twice" is `:disabled` and
        "it is running" is `:loading` — a hand-rolled button has to remember both, and antd announces both to a screen
        reader. The label is the command's own word in progress (R32).

        ★ `scanning` is the kernel's scan **or** this page's own call being served: a real scan answers the invoke rather
        than streaming events, so waiting for the store to say otherwise would leave the button live and unlabelled for the
        second and a half the network is being asked.
      -->
      <AButton
        type="primary"
        :loading="scanning"
        :disabled="scanning"
        @click="scan"
      >
        {{ scanning ? messages.devices.scanning : messages.devices.scan }}
      </AButton>
    </div>

    <ErrorNotice v-if="error" :code="error.code" :message="error.message" :actions="error.actions" @retry="repeat()" />

    <!--
      A pairing request is time-sensitive — the peer's connection is waiting on this answer — so it sits above the list rather
      than inside a device's row: the row may be scrolled away, and the prompt belongs to *this* device, not to that one.

      The code is the whole prompt. A device on the same network could otherwise ask in place of the one the user meant, and a
      comparison a person makes on two screens is what stops it.
    -->
    <div v-for="prompt in pairing.pending" :key="prompt.requestId" class="stack gap-2 rounded p-3 ring-1 ring-warning">
      <div class="row-between flex-wrap gap-2">
        <span class="text-sm font-medium">{{ messages.devices.pairingTitle }}</span>
        <span class="text-xs text-ink-dim">{{ prompt.displayName }} · {{ prompt.platform }} · {{ prompt.address }}</span>
      </div>
      <div class="row flex-wrap items-baseline gap-2">
        <span class="text-xs text-ink-dim">{{ messages.devices.pairingCode }}</span>
        <span class="text-xl font-mono">{{ prompt.verificationCode }}</span>
      </div>
      <div class="row gap-2">
        <AButton :loading="pairingBusy === prompt.requestId" @click="answer(prompt.requestId, true)">
          {{ messages.devices.pairingAccept }}
        </AButton>
        <AButton :loading="pairingBusy === prompt.requestId" @click="answer(prompt.requestId, false)">
          {{ messages.devices.pairingDecline }}
        </AButton>
      </div>
    </div>

    <!--
      The scan's own report, as three labelled facts rather than one line joined by separators (R21): "found 2 ·
      1200ms of 3000ms" is three things pretending to be one, and the join is the shape the design document retires.
    -->
    <div v-if="scanState" class="stack gap-2">
      <AProgress :percent="scanPercent" :show-info="false" />

      <dl class="m-0 row flex-wrap gap-4 text-xs">
        <div class="row gap-1">
          <dt class="m-0 text-ink-dim">
            {{ messages.devices.scanProgress }}
          </dt>
          <dd class="m-0 text-ink">
            {{ scanState.found }}
          </dd>
        </div>
        <div class="row gap-1">
          <dt class="m-0 text-ink-dim">
            {{ messages.devices.scanElapsed }}
          </dt>
          <dd class="m-0 text-ink">
            {{ scanState.elapsedMs }} ms
          </dd>
        </div>
        <div class="row gap-1">
          <dt class="m-0 text-ink-dim">
            {{ messages.devices.scanTotal }}
          </dt>
          <dd class="m-0 text-ink">
            {{ scanState.totalMs }} ms
          </dd>
        </div>
      </dl>
    </div>

    <!--
      R28: an empty state is an invitation with a next step, not a sentence — so this one carries the same action the
      header does, rather than telling the user to go and find it.

      R26: and what is below it is a list, not a grid of cards. A device is a row of facts; a screen of isomorphic
      rounded cards is the shape the design document retires.
    -->
    <AEmpty v-if="devices.cards.length === 0" :description="messages.devices.empty">
      <template #image>
        <!--
          The icon rather than antd's default illustration: that illustration draws its own light-grey palette into a
          screen whose neutrals are all one hue (§1.4), and empty states are named as the places that must not compete
          for attention (R27).
        -->
        <span class="i-carbon-devices text-xl text-ink-dim" aria-hidden="true" />
      </template>

      <AButton :loading="devices.scanning" :disabled="devices.scanning" @click="scan">
        {{ messages.devices.emptyAction }}
      </AButton>
    </AEmpty>

    <AList v-else :data-source="cards">
      <template #renderItem="{ item: card }">
        <AList.Item>
          <div class="w-full row-between flex-wrap gap-3">
            <div class="min-w-0 stack gap-0.5">
              <span class="truncate text-sm font-medium">{{ card.displayName }}</span>
              <span class="truncate text-xs text-ink-dim">{{ card.platform }}</span>
            </div>

            <!--
              Two questions, two tags: a paired device that is offline is one the user can still send to later, and an
              online device that is unpaired is one they have to pair first. The word is always inside the tag (R23),
              and the colour marks the answer that needs the user rather than decorating the ones that are fine.
            -->
            <div class="row flex-wrap gap-2">
              <ATag :color="card.paired ? 'default' : 'warning'">
                {{ card.paired ? messages.devices.paired : messages.devices.unpaired }}
              </ATag>
              <ATag :color="card.online ? 'default' : 'error'">
                {{ card.online ? messages.devices.online : messages.devices.offline }}
              </ATag>

              <!-- An address is machine-issued, which is what monospace is for (R21). -->
              <span v-if="card.address" class="text-xs text-ink-dim font-mono">{{ card.address }}</span>

              <!--
                The one action a discovered device needs. Shown only when it is unpaired: once paired, the row's work is sending, and a
                button that offered to pair again would be offering to repeat the decision the user already made.
              -->
              <AButton
                v-if="!card.paired"
                size="small"
                :loading="pairingBusy === card.deviceId"
                @click="pair(card.deviceId)"
              >
                {{ messages.devices.pair }}
              </AButton>
            </div>
          </div>
        </AList.Item>
      </template>
    </AList>
  </section>
</template>

<script setup lang="ts">
/**
 * Pairing, sending, and what is nearby.
 *
 * # Why the scan is worth a button here
 *
 * The snapshot is a *picture*, and a picture of a list of devices does not show that the list is live. The scan is the one
 * action that asks the kernel to look, so it is the only way to exercise the path the design document cares about — listen
 * first, then apply by sequence number, and re-read on a gap.
 *
 * ★ **And the scan takes options, which this page used not to send at all.** The call was `call('devices_scan_start')`
 * with no arguments, while the shell's wrapper declares `opts: ScanOptions` as required. Tauri deserializes a command's
 * arguments *before* the command runs, so the call was refused by the bridge — `command devices_scan_start missing
 * required key opts` — and a rejection that is not an `IpcError` is what the client reports as "The interface could not
 * reach the kernel". The kernel never saw the call, the reason was in `params.detail` and nothing renders that, and the
 * fake kernel answered anyway because it ignored its arguments. So the arguments are typed now, and the fake requires
 * them.
 *
 * ★ **Which scan to ask for is the kernel's answer rather than this page's guess.** The page asks for the scan the button
 * means; the shell answers it by running its **own** discovery (`src-tauri/src/discovery.rs` — the sockets are the
 * window's, the way [05 §2.1](../../design-docs/05-system-architecture.md) arranges a desktop host) and writes what it
 * hears into the list this page reads. Only a window that could not bind those sockets refuses that mode, and then the
 * page asks for the cached scan instead — which is not the page papering over a failure but the half of the refusal a
 * caller acts on.
 *
 * ★ **What that costs in this page's shape.** A real scan *answers the call*; it does not stream events, because this shell
 * publishes no kernel events yet. So the button's busy state is the page's own (`busy`, below), and the list is re-read
 * when the call returns — the progress report under the header is exercised by the fake kernel, which does publish
 * `scanUpdated`, and by a snapshot taken while a scan is in flight (the shell records one there; it clears it before it
 * answers).
 *
 * # Why paired and online are two columns
 *
 * They answer different questions. A paired device that is offline is one the user can still send to later; a device that
 * is online but unpaired is one the user has to pair first. Collapsing them into a status string loses exactly the case
 * that needs the user to act.
 */
import type { DevicesSnapshot, ScanOptions, ScanState, SuggestedAction } from '@synra/protocol'
import { Button as AButton, Empty as AEmpty, List as AList, Progress as AProgress, Tag as ATag } from 'ant-design-vue'
import { computed, onMounted, ref } from 'vue'

import ErrorNotice from '../components/ErrorNotice.vue'
import { kernel } from '../kernel'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'
import { usePairingStore } from '../stores/pairing'

const devices = useDevicesStore()
const pairing = usePairingStore()

/** The refusal itself, not its sentence: the surface needs the `code`, the message **key** and the `actions` (R33). */
const error = ref<{ code: string, message: string, actions?: readonly SuggestedAction[] | undefined } | undefined>(undefined)

/**
 * The two scans `ScanOptions` describes, as this page asks for them.
 *
 * ★ Both flags are written out rather than left to a default, because the struct on the wire requires both: it denies
 * unknown fields and has no `serde(default)`, so a payload that omitted `includePaired` would fail in the bridge exactly
 * the way the missing `opts` did.
 *
 * ★ The type comes from the contract rather than from this file. `@synra/protocol`'s `ui.ts` is the hand-written mirror of
 * `synra-ipc`'s DTOs, and importing it is what makes a field renamed in Rust fail *here*, at compile time. This page
 * declared its own copy for one round, because the package's entry point exported the shapes the interface **reads** and
 * not the argument it **sends** — a copy that would have gone on compiling while the kernel refused the payload. The
 * export was added and the copy deleted; the mirror is what a caller should be holding.
 */
const NETWORK_SCAN: ScanOptions = { includePaired: false, cachedOnly: false }

/** The scan this shell can answer: read what the daemon has already written down, and wait for nothing. */
const CACHED_SCAN: ScanOptions = { includePaired: false, cachedOnly: true }

/** Records a refusal for the notice above, which is the one caller that needs a code, a key and the actions. */
function refuse(answer: { readonly code: string, readonly message: string, readonly actions?: readonly SuggestedAction[] | undefined }): void {
  error.value = { code: answer.code, message: answer.message, actions: answer.actions }
}

/**
 * The scan in progress, or nothing.
 *
 * Read through a `computed` so the template's `v-if` narrows the same value it renders, rather than a store property
 * that is re-read between the test and the use.
 */
const scanState = computed<ScanState | undefined>(() => devices.scan)

/**
 * The devices as the list takes them.
 *
 * Copied because `AList`'s `dataSource` is typed as a mutable array and the store's list is `readonly`. The copy is
 * made once per change rather than once per render, because the store replaces the whole array whenever anything in it
 * moves.
 */
const cards = computed(() => [...devices.cards])

/**
 * How far the scan has got, as a percentage.
 *
 * Clamped rather than trusted: both numbers are the kernel's clock, and a bar that reads 130% is a control telling the
 * user something false about the machine. Zero when there is no budget to divide by.
 */
const scanPercent = computed(() => {
  const current = scanState.value
  if (current === undefined || current.totalMs <= 0)
    return 0
  return Math.min(100, Math.max(0, Math.round((current.elapsedMs / current.totalMs) * 100)))
})

/**
 * Whether a scan this page started is being served.
 *
 * ★ Why the page keeps its own flag rather than reading `devices.scanning` alone.
 *
 * That field is the **kernel's** scan, and it arrives in a snapshot — so it changes only when a snapshot is taken. A real
 * scan answers the call rather than streaming events (this shell publishes no kernel events yet; the fake kernel does, and
 * that is where the progress bar below is exercised), so between the click and the answer the store would say nothing and
 * the button would stay live and unlabelled for the second and a half the network is being asked.
 */
const busy = ref(false)

/** What the scan control shows: either scan, from the page's point of view, is "this button has been pressed". */
const scanning = computed(() => devices.scanning || busy.value)

/**
 * The call the error surface's retry repeats (R33).
 *
 * The kernel's `retry` action means "that call, again" — and every call this page makes has its refusal rendered by the
 * notice above, so the failing one is remembered beside the refusal instead of being guessed from its code.
 */
const repeat = ref<() => Promise<void>>(loadSnapshot)

/**
 * ★ The page no longer subscribes to the channel, and that is not a regression.
 *
 * `src/events.ts` owns the one subscription, judges every event by its sequence number and applies the device cards to
 * the store this page reads. What used to be here was a listener with an empty body — registered at mount, held until
 * unmount, doing nothing — because the comment at the time said the dispatcher would "do the real work" and nobody had
 * wired one. Two listeners would now mean two sequence trackers on one channel, each concluding it had missed events.
 */
onMounted(loadSnapshot)

/**
 * Reads the device list once.
 *
 * Startup and the notice's retry are the same read, which is why they are one function: the ordering the comment above
 * is about is the caller's to get right, and two copies of it would be two places to get it wrong in.
 */
/**
 * The device or the request this page is asking about, so the right control shows the busy state.
 *
 * One ref rather than a map: a person answers one prompt at a time, and two spinners on one screen would be two places the
 * same "in flight" answer lives.
 */
const pairingBusy = ref<string | undefined>(undefined)

/**
 * Asks a discovered device to pair.
 *
 * ★ Why the button is only on unpaired rows: pairing is the decision that a device is recognised, and offering it again on a
 * row that already carries the answer would ask the user to repeat something they already settled.
 */
async function pair(deviceId: string): Promise<void> {
  repeat.value = () => pair(deviceId)
  error.value = undefined
  pairingBusy.value = deviceId

  const answer = await kernel.call<null>('pairing_request', { deviceId })
  pairingBusy.value = undefined
  if (answer.ok)
    // The card's "paired" tag is the kernel's, so the list is re-read rather than patched here.
    await loadSnapshot()
  else
    refuse(answer.error)
}

/** Answers a request that is waiting, then forgets it: whichever side settled it, the prompt is over. */
async function answer(requestId: string, accept: boolean): Promise<void> {
  repeat.value = () => answer(requestId, accept)
  error.value = undefined
  pairingBusy.value = requestId

  const settled = await kernel.call<null>('pairing_respond', { requestId, accept })
  pairingBusy.value = undefined
  if (!settled.ok) {
    refuse(settled.error)
    return
  }
  pairing.settle(requestId)
  await loadSnapshot()
}

async function loadSnapshot(): Promise<void> {
  repeat.value = loadSnapshot
  error.value = undefined

  const answer = await kernel.call<DevicesSnapshot>('devices_snapshot')
  if (answer.ok)
    devices.applySnapshot(answer.value)
  else
    refuse(answer.error)
}

/**
 * Starts the scan, in the mode this kernel can answer.
 *
 * ★ The second call is not a fallback for a failure; it is the kernel's own instruction. A shell that could not bind the
 * discovery sockets refuses the network scan with `unsupported` — no network, a firewall, a sandbox — and its reason says
 * the known devices are still listed, and `params` is not something the interface renders (R34). So the half of that
 * refusal a caller acts on is the code it branches by, which is what the client's header says codes are for.
 */
async function scan(): Promise<void> {
  repeat.value = scan
  error.value = undefined
  busy.value = true

  try {
    const started = await kernel.call<string>('devices_scan_start', { opts: NETWORK_SCAN })
    if (!started.ok) {
      if (started.error.code !== 'unsupported') {
        refuse(started.error)
        return
      }

      const cached = await kernel.call<string>('devices_scan_start', { opts: CACHED_SCAN })
      if (!cached.ok) {
        refuse(cached.error)
        return
      }
    }

    // ★ The list is where a scan shows itself, so it is re-read whichever mode answered the call.
    //
    // A real scan writes what it heard into the same table this read uses before it answers, and a cached scan reads that
    // table — and nothing else would tell this page the table changed, because the shell publishes no kernel events yet.
    await loadSnapshot()
  }
  finally {
    busy.value = false
  }
}
</script>
