<template>
  <details :open="expanded" class="min-w-0 stack gap-2 [&_.ant-btn]:min-h-11 md:[&_.ant-btn]:min-h-6" data-plugin-audit @toggle="toggle">
    <summary ref="summary" class="min-h-11 cursor-pointer py-3 text-sm text-ink-dim">
      {{ messages.plugins.audit.title }}
    </summary>
    <ErrorNotice v-if="error" :code="error.code" :message="error.message" :actions="error.actions" @retry="retryAudit" />
    <p v-if="loading" role="status" class="m-0 text-xs text-ink-dim">
      {{ messages.plugins.audit.loading }}
    </p>
    <ul v-if="rows?.length" class="m-0 stack list-none gap-3 p-0 text-xs">
      <li v-for="(entry, index) in rows" :key="`${entry.atMs}-${entry.capability}-${index}`" class="min-w-0 stack gap-1">
        <div class="min-w-0 row flex-wrap gap-2">
          <span class="min-w-0 whitespace-normal break-all font-mono">{{ entry.capability }}</span>
          <span :class="entry.allowed ? 'text-ink-dim' : 'text-danger'">
            {{ entry.allowed ? messages.plugins.audit.allowed : messages.plugins.audit.refused }}
          </span>
          <span class="text-ink-dim">{{ formatTime(entry.atMs) }}</span>
        </div>
        <span v-if="!entry.allowed && entry.reason" class="break-all text-ink-dim font-mono">{{ entry.reason }}</span>
        <details>
          <summary class="min-h-11 cursor-pointer py-3 text-ink-dim">
            {{ messages.plugins.audit.details }}
          </summary>
          <div class="min-w-0 stack gap-1 pt-1 text-ink-dim">
            <time :datetime="new Date(entry.atMs).toISOString()" class="break-all">{{ new Date(entry.atMs).toISOString() }}</time>
            <span v-if="entry.elapsedMs !== undefined">{{ messages.plugins.audit.elapsed(entry.elapsedMs) }}</span>
            <span v-if="entry.traceId" class="break-all font-mono">{{ messages.plugins.audit.trace }} {{ entry.traceId }}</span>
          </div>
        </details>
      </li>
    </ul>
    <p v-else-if="rows !== undefined && !loading && !error" class="m-0 text-xs text-ink-dim">
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
    <div v-if="confirming" class="stack gap-2 border border-line rounded-surface p-3" role="group" :aria-label="messages.plugins.audit.clear">
      <p class="m-0 text-xs">
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
  </details>
</template>

<script setup lang="ts">
import { Button as AButton } from 'ant-design-vue'
import { nextTick, ref, toRef, watch } from 'vue'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { kernel } from '../../kernel'
import { formatTime } from '../../locale-format'
import { messages } from '../../messages'
import { usePluginAudit } from './audit-state'

const props = defineProps<{ pluginId: string, active: boolean }>()
const { expanded, rows, done, error, busy, loading, clearing, setExpanded, loadMore, clear, retry, retryOperation } = usePluginAudit(kernel, toRef(props, 'pluginId'), toRef(props, 'active'))
const confirming = ref(false)
const summary = ref<HTMLElement>()
const clearButton = ref<{ $el: HTMLButtonElement }>()
const cancelButton = ref<{ $el: HTMLButtonElement }>()

watch([expanded, () => props.pluginId], () => {
  confirming.value = false
})

function toggle(event: Event): void {
  void setExpanded((event.target as HTMLDetailsElement).open)
}

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
