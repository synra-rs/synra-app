<style scoped>
.home-transfer-actions {
  min-width: 8.5rem;
  justify-content: flex-end;
  opacity: 0;
  transition: opacity 120ms ease;
}

.home-transfer-row:hover .home-transfer-actions,
.home-transfer-row:focus-within .home-transfer-actions {
  opacity: 1;
}

@media (pointer: coarse), (prefers-reduced-motion: reduce) {
  .home-transfer-actions {
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .home-transfer-actions {
    transition: none;
  }
}

@media (max-width: 40rem) {
  .home-transfer-row {
    align-items: flex-start;
  }

  .home-transfer-actions {
    width: 100%;
    min-width: 0;
    justify-content: flex-start;
  }
}
</style>

<template>
  <section class="mx-auto pb-4 stack gap-6 max-w-3xl min-w-0" data-page-width="reading">
    <PageHeader :title="messages.home.title">
      <AButton @click="router.push('/devices')">
        {{ wire.state === 'none' ? messages.home.wire.action : messages.home.inFlight.action }}
      </AButton>
    </PageHeader>
    <ErrorNotice v-if="error" :code="error.code" :actions="error.actions" @retry="load" />
    <section class="pt-4 border-t border-line stack gap-3 min-w-0" :aria-label="messages.home.wire.label">
      <div class="row-between flex-wrap gap-2">
        <span class="text-sm text-ink-dim">{{ messages.home.wire.label }}</span>
        <span class="text-base font-medium">{{ state.headline }}</span>
      </div>
      <div class="row gap-3 min-w-0">
        <span class="text-base max-w-1/3 min-w-0 truncate" :title="devices.identity?.displayName">
          {{ devices.identity?.displayName ?? messages.app.name }}
        </span>
        <span class="rounded-pill bg-line flex-1 h-0.5 max-w-24 min-w-6 relative" aria-hidden="true">
          <span class="rounded-pill bg-primary start-0 inset-y-0 absolute" :class="[wireWidth, wireLive && 'shadow-glow']" />
        </span>
        <span class="text-base text-end max-w-1/3 min-w-0 truncate" :class="wirePeerName ? 'text-ink' : 'text-ink-dim'" :title="wirePeerName">
          {{ wirePeerName ?? messages.home.wire.action }}
        </span>
      </div>
      <p v-if="'detail' in state" class="text-sm text-ink-dim m-0">
        {{ state.detail }}
      </p>
    </section>
    <section class="pt-4 border-t border-line stack gap-3 min-w-0">
      <h2 class="text-base font-medium m-0">
        {{ messages.home.inFlight.title }}
      </h2>
      <p v-if="transfers.length === 0" class="text-base text-ink-dim m-0 py-2">
        {{ messages.home.inFlight.empty }}
      </p>
      <AList v-else size="small">
        <AList.Item v-for="transfer in transfers" :key="transfer.transferId" :style="{ paddingInline: 0 }">
          <div class="stack gap-2 min-w-0 w-full">
            <div class="home-transfer-row row-between gap-3">
              <div class="stack gap-0.5 min-w-0">
                <span class="text-base truncate">{{ transfer.peerName }}</span>
                <span v-if="transfer.fileName" class="text-sm text-ink-dim truncate">{{ transfer.fileName }}</span>
              </div>
              <div class="home-transfer-actions row gap-2">
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
    <section class="pt-4 border-t border-line stack gap-3 min-w-0">
      <h2 class="text-base font-medium m-0">
        {{ messages.home.received.title }}
      </h2>
      <p v-if="intents.length === 0" class="text-base text-ink-dim m-0 py-2">
        {{ messages.home.received.empty }}
      </p>
      <AList v-else size="small">
        <AList.Item v-for="intent in intents" :key="intent.intentId" :style="{ paddingInline: 0 }">
          <div class="stack gap-2 min-w-0 w-full">
            <div class="row flex-wrap gap-3 min-w-0">
              <ATag class="m-0" :color="intentStateColor(intent.state)">
                {{ intentStateLabel(intent.state) }}
              </ATag>
              <span v-if="intent.pluginId" class="text-sm text-ink-dim font-mono max-w-full min-w-0 break-all">{{ intent.pluginId }}</span>
            </div>
            <p v-if="intent.retry" class="text-sm text-ink-dim m-0">
              {{ retryLabel(intent.retry) }}
            </p>
            <div v-for="candidate in intent.candidates" :key="candidate.deviceId" class="row-between gap-2">
              <span class="text-sm min-w-0 truncate">
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
            <p v-if="intent.error" class="text-sm m-0">
              <span class="text-error">{{ intentError(intent) }}</span>
            </p>
            <div v-if="intent.traceId || intent.startedAtMs !== undefined" class="stack gap-1 min-w-0">
              <button
                type="button"
                class="text-sm text-ink-dim px-0 focusable border-0 bg-transparent min-h-11 self-start hover:text-ink md:min-h-8"
                :aria-expanded="expanded === intent.intentId"
                @click="toggleExpanded(intent.intentId)"
              >
                {{ messages.home.received.details }}
              </button>
              <div v-if="expanded === intent.intentId" class="text-sm stack gap-1">
                <div v-if="intent.startedAtMs !== undefined" class="row flex-wrap gap-2 items-baseline">
                  <span class="text-ink-dim">{{ messages.home.received.startedAt }}</span>
                  <span class="font-mono min-w-0 break-all">{{ formatClock(intent.startedAtMs) }}</span>
                </div>
                <div v-if="intent.traceId" class="row flex-wrap gap-2 items-center">
                  <span class="text-ink-dim">{{ messages.home.received.trace }}</span>
                  <span class="font-mono min-w-0 select-text break-all" :data-testid="`trace-${intent.intentId}`">{{ intent.traceId }}</span>
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
    <section class="pt-4 border-t border-line stack gap-3 min-w-0">
      <h2 class="text-base font-medium m-0">
        {{ messages.home.receivedFiles.title }}
      </h2>
      <p v-if="received.length === 0" class="text-base text-ink-dim m-0 py-2">
        {{ messages.home.receivedFiles.empty }}
      </p>
      <AList v-else size="small">
        <AList.Item v-for="file in received" :key="file.path" :style="{ paddingInline: 0 }">
          <div class="row-between gap-3">
            <div class="stack gap-0.5 min-w-0">
              <span class="text-base truncate" :title="file.fileName">{{ file.fileName }}</span>
              <span class="text-sm text-ink-dim font-mono break-all" :title="file.path">{{ file.path }}</span>
              <span class="text-sm text-ink-dim" :title="formatDateTime(file.receivedAtMs)">
                {{ messages.home.receivedFiles.receivedAt }} {{ formatDateTime(file.receivedAtMs) }}
              </span>
            </div>
            <span class="text-sm text-ink-dim shrink-0">{{ formatBytes(file.bytes) }}</span>
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
import { useHomePage } from '../composables/use-home-page'
import { messages } from '../messages'
import { formatBytes } from '../utils/format'
import { formatDateTime } from '../utils/locale-format'

const { router, devices, askBeforeCancel, error, received, cancelling, percentOf, progressLabel, directionOf, stateLabel, cancelTransfer, wire, state, wirePeerName, wireWidth, wireLive, load, expanded, copiedTrace, selectedTrace, toggleExpanded, copyTrace, deciding, accept, decline, canDecline, deviceName, intentStateLabel, intentStateColor, retryLabel, formatClock, intentError, intents, transfers } = useHomePage()
</script>
