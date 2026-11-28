<template>
  <section class="stack gap-2 min-w-0" :aria-label="messages.plugins.audit.title" data-plugin-audit>
    <h3 ref="summary" tabindex="-1" class="sr-only">
      {{ messages.plugins.audit.title }}
    </h3>
    <ErrorNotice v-if="error" :code="error.code" :actions="error.actions" @retry="retryAudit" />
    <p v-if="loading" role="status" class="text-sm text-ink-dim m-0">
      {{ messages.plugins.audit.loading }}
    </p>
    <ul v-if="rows?.length" class="text-sm m-0 p-0 list-none stack gap-3">
      <li v-for="(entry, index) in rows" :key="`${entry.atMs}-${entry.capability}-${index}`" class="stack gap-1 min-w-0">
        <div class="row flex-wrap gap-2 min-w-0">
          <span class="font-mono min-w-0 whitespace-normal break-all">{{ entry.capability }}</span>
          <span :class="entry.allowed ? 'text-ink-dim' : 'text-danger'">
            {{ entry.allowed ? messages.plugins.audit.allowed : messages.plugins.audit.refused }}
          </span>
          <span class="text-ink-dim">{{ formatTime(entry.atMs) }}</span>
        </div>
        <span v-if="!entry.allowed && entry.reason" class="text-ink-dim font-mono break-all">{{ entry.reason }}</span>
        <details>
          <summary class="text-ink-dim py-3 min-h-11 cursor-pointer">
            {{ messages.plugins.audit.details }}
          </summary>
          <div class="text-ink-dim pt-1 stack gap-1 min-w-0">
            <time :datetime="new Date(entry.atMs).toISOString()" class="break-all">{{ new Date(entry.atMs).toISOString() }}</time>
            <span v-if="entry.elapsedMs !== undefined">{{ messages.plugins.audit.elapsed(entry.elapsedMs) }}</span>
            <span v-if="entry.traceId" class="font-mono break-all">{{ messages.plugins.audit.trace }} {{ entry.traceId }}</span>
          </div>
        </details>
      </li>
    </ul>
    <p v-else-if="rows !== undefined && !loading && !error" class="text-sm text-ink-dim m-0">
      {{ messages.plugins.audit.empty }}
    </p>
    <div v-if="rows?.length" class="row flex-wrap gap-2">
      <AButton v-if="!done" size="small" :loading="busy === 'more'" :disabled="busy !== undefined || confirming" @click="loadMore">
        {{ messages.plugins.audit.more }}
      </AButton>
      <AButton ref="clearButton" size="small" danger :disabled="busy !== undefined || confirming" @click="askClear">
        {{ messages.plugins.audit.clear }}
      </AButton>
    </div>
    <div v-if="confirming" class="p-3 border border-line rounded-surface stack gap-2" role="group" :aria-label="messages.plugins.audit.clear">
      <p class="text-sm m-0">
        {{ messages.plugins.audit.clearDetail }}
      </p>
      <div class="row gap-2">
        <AButton ref="cancelButton" size="small" :disabled="clearing" @click="cancelClear">
          {{ messages.plugins.audit.cancel }}
        </AButton>
        <AButton size="small" danger :loading="clearing" @click="confirmClear">
          {{ messages.plugins.audit.confirmClear }}
        </AButton>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { Button as AButton } from 'ant-design-vue'
import { nextTick, ref, toRef, watch } from 'vue'
import { kernel } from '../../app/kernel'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { usePluginAudit } from '../../features/plugins/audit-state'
import { messages } from '../../messages'
import { formatTime } from '../../utils/locale-format'

const props = defineProps<{ pluginId: string, active: boolean }>()
const { rows, done, error, busy, loading, clearing, setExpanded, loadMore, clear, retry, retryOperation } = usePluginAudit(kernel, toRef(props, 'pluginId'), toRef(props, 'active'))
const confirming = ref(false)
const summary = ref<HTMLElement>()
const clearButton = ref<{ $el: HTMLButtonElement }>()
const cancelButton = ref<{ $el: HTMLButtonElement }>()

watch([
  () => props.active,
  () => props.pluginId
], () => {
  confirming.value = false
})

watch(() => props.active, active => void setExpanded(active), { immediate: true })

async function askClear(): Promise<void> {
  confirming.value = true
  await nextTick()
  cancelButton.value?.$el.focus()
}

async function cancelClear(): Promise<void> {
  confirming.value = false
  await nextTick()
  clearButton.value?.$el.focus()
}

async function confirmClear(): Promise<void> {
  if (await clear()) {
    confirming.value = false
    await nextTick()
    summary.value?.focus()
  }
}

async function retryAudit(): Promise<void> {
  if (retryOperation.value === 'clear') {
    if (confirming.value)
      await confirmClear()
    else
      await askClear()
  }
  else {
    await retry()
  }
}
</script>
