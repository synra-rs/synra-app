<template>
  <section class="mx-auto max-w-3xl min-w-0 stack gap-6 pb-4" data-page-width="reading">
    <PageHeader :title="messages.home.title">
      <AButton @click="router.push('/devices')">
        {{ wire.state === 'none' ? messages.home.wire.action : messages.home.inFlight.action }}
      </AButton>
    </PageHeader>
    <ErrorNotice v-if="error" :code="error.code" :actions="error.actions" @retry="load" />
    <section class="min-w-0 stack gap-3 border-t border-line pt-4" :aria-label="messages.home.wire.label">
      <div class="row-between flex-wrap gap-2">
        <span class="text-sm text-ink-dim">{{ messages.home.wire.label }}</span>
        <span class="text-base font-medium">{{ state.headline }}</span>
      </div>
      <div class="min-w-0 row gap-3">
        <span class="max-w-1/3 min-w-0 truncate text-base" :title="devices.identity?.displayName">
          {{ devices.identity?.displayName ?? messages.app.name }}
        </span>
        <span class="relative h-0.5 max-w-24 min-w-6 flex-1 rounded-pill bg-line" aria-hidden="true">
          <span class="absolute start-0 inset-y-0 rounded-pill bg-primary" :class="[wireWidth, wireLive && 'shadow-glow']" />
        </span>
        <span class="max-w-1/3 min-w-0 truncate text-end text-base" :class="wirePeerName ? 'text-ink' : 'text-ink-dim'" :title="wirePeerName">
          {{ wirePeerName ?? messages.home.wire.action }}
        </span>
      </div>
      <p v-if="'detail' in state" class="m-0 text-sm text-ink-dim">
        {{ state.detail }}
      </p>
    </section>
    <section class="min-w-0 stack gap-3 border-t border-line pt-4">
      <h2 class="m-0 text-base font-medium">
        {{ messages.home.inFlight.title }}
      </h2>
      <p v-if="transfers.length === 0" class="m-0 py-2 text-base text-ink-dim">
        {{ messages.home.inFlight.empty }}
      </p>
      <AList v-else size="small">
        <AList.Item v-for="transfer in transfers" :key="transfer.transferId" :style="{ paddingInline: 0 }">
          <div class="min-w-0 w-full stack gap-2">
            <div class="row-between gap-3">
              <div class="min-w-0 stack gap-0.5">
                <span class="truncate text-base">{{ transfer.peerName }}</span>
                <span v-if="transfer.fileName" class="truncate text-sm text-ink-dim">{{ transfer.fileName }}</span>
              </div>
              <div class="row gap-2">
                <ATag class="m-0">
                  {{ directionOf(transfer) }}
                </ATag>
                <ATag class="m-0">
                  {{ stateLabel(transfer) }}
                </ATag>
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
            <div class="row gap-3">
              <AProgress
                class="flex-1"
                :percent="percentOf(transfer)"
                role="progressbar"
                :aria-valuenow="percentOf(transfer)"
                aria-valuemin="0"
                aria-valuemax="100"
                :aria-valuetext="progressLabel(transfer)"
                size="small"
              />
              <span class="text-sm text-ink-dim">{{ progressLabel(transfer) }}</span>
            </div>
          </div>
        </AList.Item>
      </AList>
    </section>
    <section class="min-w-0 stack gap-3 border-t border-line pt-4">
      <h2 class="m-0 text-base font-medium">
        {{ messages.home.received.title }}
      </h2>
      <p v-if="intents.length === 0" class="m-0 py-2 text-base text-ink-dim">
        {{ messages.home.received.empty }}
      </p>
      <AList v-else size="small">
        <AList.Item v-for="intent in intents" :key="intent.intentId" :style="{ paddingInline: 0 }">
          <div class="min-w-0 w-full stack gap-2">
            <div class="min-w-0 row flex-wrap gap-3">
              <ATag class="m-0" :color="intentStateColor(intent.state)">
                {{ intentStateLabel(intent.state) }}
              </ATag>
              <span v-if="intent.pluginId" class="max-w-full min-w-0 break-all text-sm text-ink-dim font-mono">{{ intent.pluginId }}</span>
            </div>
            <p v-if="intent.retry" class="m-0 text-sm text-ink-dim">
              {{ retryLabel(intent.retry) }}
            </p>
            <div v-for="candidate in intent.candidates" :key="candidate.deviceId" class="row-between gap-2">
              <span class="min-w-0 truncate text-sm">
                {{ candidate.title }} · {{ deviceName(candidate.deviceId) }}
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
            <p v-if="intent.error" class="m-0 text-sm">
              <span class="text-error">{{ intentError(intent) }}</span>
            </p>
            <div v-if="intent.traceId || intent.startedAtMs !== undefined" class="min-w-0 stack gap-1">
              <button
                type="button"
                class="min-h-11 self-start border-0 bg-transparent px-0 text-sm text-ink-dim focusable md:min-h-8 hover:text-ink"
                :aria-expanded="expanded === intent.intentId"
                @click="toggleExpanded(intent.intentId)"
              >
                {{ messages.home.received.details }}
              </button>
              <div v-if="expanded === intent.intentId" class="stack gap-1 text-sm">
                <div v-if="intent.startedAtMs !== undefined" class="row flex-wrap items-baseline gap-2">
                  <span class="text-ink-dim">{{ messages.home.received.startedAt }}</span>
                  <span class="min-w-0 break-all font-mono">{{ formatClock(intent.startedAtMs) }}</span>
                </div>
                <div v-if="intent.traceId" class="row flex-wrap items-center gap-2">
                  <span class="text-ink-dim">{{ messages.home.received.trace }}</span>
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
    <section class="min-w-0 stack gap-3 border-t border-line pt-4">
      <h2 class="m-0 text-base font-medium">
        {{ messages.home.receivedFiles.title }}
      </h2>
      <p v-if="received.length === 0" class="m-0 py-2 text-base text-ink-dim">
        {{ messages.home.receivedFiles.empty }}
      </p>
      <AList v-else size="small">
        <AList.Item v-for="file in received" :key="file.path" :style="{ paddingInline: 0 }">
          <div class="row-between gap-3">
            <div class="min-w-0 stack gap-0.5">
              <span class="truncate text-base" :title="file.fileName">{{ file.fileName }}</span>
              <span class="break-all text-sm text-ink-dim font-mono" :title="file.path">{{ file.path }}</span>
              <span class="text-sm text-ink-dim" :title="formatDateTime(file.receivedAtMs)">
                {{ messages.home.receivedFiles.receivedAt }} {{ formatDateTime(file.receivedAtMs) }}
              </span>
            </div>
            <span class="shrink-0 text-sm text-ink-dim">{{ formatBytes(file.bytes) }}</span>
          </div>
        </AList.Item>
      </AList>
    </section>
  </section>
</template>

<script setup lang="ts">
import { Button as AButton, List as AList, Popconfirm as APopconfirm, Progress as AProgress, Tag as ATag } from 'ant-design-vue'
import ErrorNotice from '../components/ErrorNotice.vue'
import PageHeader from '../components/PageHeader.vue'
import { formatBytes } from '../format'
import { formatDateTime } from '../locale-format'
import { messages } from '../messages'
import { useHomePage } from './home/use-home-page'

const { router, devices, askBeforeCancel, error, received, cancelling, percentOf, progressLabel, directionOf, stateLabel, cancelTransfer, wire, state, wirePeerName, wireWidth, wireLive, load, expanded, copiedTrace, selectedTrace, toggleExpanded, copyTrace, deciding, accept, decline, canDecline, deviceName, intentStateLabel, intentStateColor, retryLabel, formatClock, intentError, intents, transfers } = useHomePage()
</script>
