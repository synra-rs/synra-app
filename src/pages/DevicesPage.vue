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
      -->
      <AButton
        type="primary"
        :loading="devices.scanning"
        :disabled="devices.scanning"
        @click="scan"
      >
        {{ devices.scanning ? messages.devices.scanning : messages.devices.scan }}
      </AButton>
    </div>

    <ErrorNotice v-if="error" :code="error.code" :message="error.message" :actions="error.actions" @retry="repeat()" />

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
 * The snapshot is a *picture*, and a picture of a list of devices does not show that the list is live. The scan is the
 * one action whose whole effect arrives as events, so it is the only way to exercise the path the design document cares
 * about — listen first, then apply by sequence number, and re-read on a gap.
 *
 * ★ **And the scan takes options, which this page used not to send at all.** The call was `call('devices_scan_start')`
 * with no arguments, while the shell's wrapper declares `opts: ScanOptions` as required. Tauri deserializes a command's
 * arguments *before* the command runs, so the call was refused by the bridge — `command devices_scan_start missing
 * required key opts` — and a rejection that is not an `IpcError` is what the client reports as "The interface could not
 * reach the kernel". The kernel never saw the call, the reason was in `params.detail` and nothing renders that, and the
 * fake kernel answered anyway because it ignored its arguments. So the arguments are typed now, and the fake requires
 * them.
 *
 * ★ **Which scan to ask for is the kernel's answer rather than this page's guess.** This shell answers only the
 * `cachedOnly` mode: discovery belongs to the daemon, which is already writing what it hears into the list this page
 * reads (`src-tauri/src/commands/devices.rs` says so, and what is missing is the shell's network half rather than a
 * request). So the page asks for the scan the button means, and when the kernel refuses that *mode* it asks for the cached
 * scan — which is not the page papering over a refusal: the kernel's own refusal tells the caller to do exactly that.
 * Against the fake kernel the first path is real: `devices_scan_start` makes the fake publish `scanUpdated` and then
 * `deviceCardUpdated`, and the dispatcher's `SequenceTracker` judges every one of them.
 *
 * # Why paired and online are two columns
 *
 * They answer different questions. A paired device that is offline is one the user can still send to later; a device that
 * is online but unpaired is one the user has to pair first. Collapsing them into a status string loses exactly the case
 * that needs the user to act.
 */
import type { DevicesSnapshot, ScanState, SuggestedAction } from '@synra/protocol'
import { Button as AButton, Empty as AEmpty, List as AList, Progress as AProgress, Tag as ATag } from 'ant-design-vue'
import { computed, onMounted, onUnmounted, ref } from 'vue'

import ErrorNotice from '../components/ErrorNotice.vue'
import { KERNEL_EVENT_CHANNEL } from '../ipc/channel'
import { kernel } from '../kernel'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'

const devices = useDevicesStore()

/** The refusal itself, not its sentence: the surface needs the `code`, the message **key** and the `actions` (R33). */
const error = ref<{ code: string, message: string, actions?: readonly SuggestedAction[] | undefined } | undefined>(undefined)

/**
 * The two scans `ScanOptions` describes, as this page asks for them.
 *
 * ★ Both flags are written out rather than left to a default, because the struct on the wire requires both: it denies
 * unknown fields and has no `serde(default)`, so a payload that omitted `includePaired` would fail in the bridge exactly
 * the way the missing `opts` did. The type is the contract's own — `@synra/protocol` generates it from `synra-ipc` — so
 * the day a flag is added this file stops compiling rather than sending a payload the kernel refuses.
 */
/**
 * ★ **`devices_scan_start`'s options, declared here because the protocol does not export them.**
 *
 * The shell's own wrapper takes `opts: ScanOptions` (see the note above `loadSnapshot`), so the shape is the kernel's
 * and not this page's invention — but `@synra/protocol`'s generated types do not carry it, and a page cannot import a
 * name that was never generated. An earlier version of this file imported it anyway, which typechecked only while
 * `vue-tsc`'s incremental cache said the project was up to date.
 *
 * When the generated types grow this DTO, delete this and import it (the same one-source rule as everything else on
 * the wire).
 */
interface ScanOptions {
  readonly includePaired: boolean
  readonly cachedOnly: boolean
}

const NETWORK_SCAN: ScanOptions = { includePaired: false, cachedOnly: false }

/** The scan this shell can answer: read what the daemon has already written down, and wait for nothing. */
const CACHED_SCAN: ScanOptions = { includePaired: false, cachedOnly: true }

/** Records a refusal for the notice above, which is the one caller that needs a code, a key and the actions. */
function refuse(answer: { readonly code: string, readonly message: string, readonly actions?: readonly SuggestedAction[] | undefined }): void {
  error.value = { code: answer.code, message: answer.message, actions: answer.actions }
}

let stop: (() => void) | undefined

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
 * The call the error surface's retry repeats (R33).
 *
 * The kernel's `retry` action means "that call, again" — and every call this page makes has its refusal rendered by the
 * notice above, so the failing one is remembered beside the refusal instead of being guessed from its code.
 */
const repeat = ref<() => Promise<void>>(loadSnapshot)

onMounted(async () => {
  await loadSnapshot()

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

/**
 * Reads the device list once.
 *
 * Startup and the notice's retry are the same read, which is why they are one function: the ordering the comment above
 * is about is the caller's to get right, and two copies of it would be two places to get it wrong in.
 */
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
 * ★ The second call is not a fallback for a failure; it is the kernel's own instruction. A shell with no discovery half
 * refuses the network scan with `unsupported` and a reason that ends "Ask for a cached scan to see what is already
 * known", and `params` is not something the interface renders (R34) — so the half of that refusal a caller acts on is the
 * code it branches by, which is what the client's header says codes are for.
 */
async function scan(): Promise<void> {
  repeat.value = scan
  error.value = undefined

  const started = await kernel.call<string>('devices_scan_start', { opts: NETWORK_SCAN })
  if (started.ok) {
    // A scan that is really running reports itself as events, which is the path this button exists to exercise.
    return
  }
  if (started.error.code !== 'unsupported') {
    refuse(started.error)
    return
  }

  const cached = await kernel.call<string>('devices_scan_start', { opts: CACHED_SCAN })
  if (!cached.ok) {
    refuse(cached.error)
    return
  }
  // A cached scan is over before it answers and publishes nothing — there is no event to wait for — so the list it read
  // is what shows it happened.
  await loadSnapshot()
}
</script>
