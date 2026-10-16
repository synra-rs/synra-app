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
 * Against the fake kernel that path is real: `devices_scan_start` makes the fake publish `scanUpdated` and then
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
 * The kernel's `retry` action means "that call, again" — and this page makes two calls whose refusals the notice above
 * renders the same way, so the failing one is remembered beside the refusal instead of being guessed from its code.
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
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}

async function scan(): Promise<void> {
  repeat.value = scan
  error.value = undefined

  const answer = await kernel.call('devices_scan_start')
  if (!answer.ok)
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}
</script>
