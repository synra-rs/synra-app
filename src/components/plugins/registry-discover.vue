<template>
  <section class="min-w-0 stack gap-4" :aria-label="messages.plugins.catalog.title">
    <div class="flex flex-col gap-2 sm:flex-row">
      <div class="min-w-0 row flex-1 gap-2">
        <AInput v-model:value="text" class="min-w-0 flex-1" :aria-label="messages.plugins.catalog.placeholder" :placeholder="messages.plugins.catalog.placeholder" allow-clear @press-enter="submit" />
        <AButton type="primary" :loading="searching" :disabled="!text.trim() || busy" @click="search">
          {{ messages.plugins.catalog.action }}
        </AButton>
      </div>
      <ASelect v-model:value="sort" :options="sortOptions" :aria-label="messages.plugins.catalog.sort" class="w-32 self-end" :disabled="busy" />
    </div>
    <div v-if="missingRegistry" class="stack items-start gap-2">
      <p class="m-0 text-base text-ink-dim">
        {{ messages.plugins.workflow.registryMissing }}
      </p>
      <AButton @click="router.push('/settings?category=plugins')">
        {{ messages.actions.openSettings }}
      </AButton>
    </div>
    <ErrorNotice v-else-if="error" :code="error.code" :detail="error.detail" :actions="error.actions" @retry="retry" />
    <p v-if="searching" class="m-0 text-base text-ink-dim" role="status">
      {{ messages.plugins.workflow.loading }}
    </p>
    <article v-for="item in results" :key="item.pluginId" class="min-w-0 row-between gap-3 border-b border-line py-3">
      <div class="min-w-0 stack gap-1">
        <span class="truncate text-base font-medium">{{ item.title }}</span>
        <span v-if="item.description" class="text-base text-ink-dim">{{ item.description }}</span>
        <span class="break-all text-sm text-ink-dim">{{ item.publisher }} · {{ item.version }} · {{ item.pluginId }}</span>
      </div>
      <AButton size="small" @click="show(item.pluginId, item.installed)">
        {{ store.plugin(item.pluginId) || item.installed ? messages.plugins.categories.installed : messages.plugins.workflow.details }}
      </AButton>
    </article>
    <p v-if="searched && !results.length && !error && !busy" class="m-0 text-base text-ink-dim">
      {{ messages.plugins.catalog.empty }}
    </p>
    <p v-else-if="!searched && !busy && !error" class="m-0 text-base text-ink-dim">
      {{ messages.plugins.catalog.idle }}
    </p>
    <AButton v-if="hasMore" class="self-start" :loading="loadingMore" :disabled="busy" @click="loadMore">
      {{ messages.plugins.catalog.loadMore }}
    </AButton>
    <RegistryInstallDialog v-if="selected" :key="selected" :plugin-id="selected" @close="selected = ''" @installed="installed" />
  </section>
</template>

<script setup lang="ts">
import type { InstallReport } from '@synra/protocol'
import { Button as AButton, Input as AInput, Select as ASelect } from 'ant-design-vue'
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { kernel } from '../../app/kernel'
import ErrorNotice from '../../components/ErrorNotice.vue'
import { useCatalog } from '../../features/plugins/catalog'
import { messages } from '../../messages'
import { usePluginsStore } from '../../stores/plugins'
import RegistryInstallDialog from './registry-install-dialog.vue'

const emit = defineEmits<{ installed: [report: InstallReport], details: [id: string] }>()
const router = useRouter()
const store = usePluginsStore()
const { text, sort, results, searched, error, hasMore, searching, loadingMore, busy, search, loadMore, retry } = useCatalog(kernel)
const selected = ref('')
const sortOptions = computed(() => [{ label: messages.plugins.catalog.byRelevance, value: 'relevance' }, { label: messages.plugins.catalog.byName, value: 'name' }])
const missingRegistry = computed(() => error.value?.message.includes('no registry is configured'))
function submit(event: KeyboardEvent): void {
  if (!event.isComposing && event.keyCode !== 229)
    void search()
}
function show(id: string, installed: boolean): void {
  if (store.plugin(id) || installed)
    emit('details', id)
  else
    selected.value = id
}
function installed(report: InstallReport): void {
  selected.value = ''
  emit('installed', report)
}
</script>
