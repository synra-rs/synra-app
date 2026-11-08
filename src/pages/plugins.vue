<template>
  <section class="mx-auto max-w-[1200px] min-w-0 stack gap-5 pb-4" data-page-width="workspace">
    <PageHeader :title="messages.plugins.title">
      <AButton data-testid="publishers-toggle" @click="publishersOpen = true">
        {{ messages.plugins.publishers.manage(attentionCount) }}
      </AButton>
      <AButton ref="installToggle" type="primary" data-testid="install-toggle" @click="installOpen = true">
        {{ messages.plugins.workflow.localInstall }}
      </AButton>
    </PageHeader>
    <ASegmented v-model:value="category" class="self-start" :options="categories" data-testid="plugin-category" />
    <div v-if="library.note" class="stack gap-2 border-b border-line pb-3" role="status">
      <p class="m-0 text-base">
        {{ library.note }}
      </p>
      <p v-for="warning in library.warnings" :key="warning" class="m-0 text-sm text-warn">
        {{ warning }}
      </p>
      <AButton v-if="lastInstalled" size="small" class="self-start" @click="showDetails(lastInstalled.pluginId, 'permissions', 'review')">
        {{ messages.plugins.reviewPermissions }}
      </AButton>
    </div>
    <div v-if="pendingPublisher" class="stack gap-2 border border-line rounded-control p-3">
      <p class="m-0 text-base">
        {{ messages.plugins.publisherPrompt.title }}
      </p>
      <p class="m-0 break-all text-sm text-ink-dim font-mono">
        {{ pendingPublisher.id }} · {{ pendingPublisher.fingerprint }}
      </p>
      <div class="row flex-wrap gap-2">
        <AButton size="small" :disabled="!!publishers.deciding" @click="decidePending('trusted')">
          {{ messages.plugins.publisherPrompt.trust }}
        </AButton>
        <AButton size="small" :disabled="!!publishers.deciding" @click="decidePending('revoked')">
          {{ messages.plugins.publisherPrompt.revoke }}
        </AButton>
        <AButton type="text" size="small" @click="pendingPublisher = undefined">
          {{ messages.plugins.publisherPrompt.later }}
        </AButton>
      </div>
      <ErrorNotice v-if="publishers.errors[pendingPublisher.id]" :code="publishers.errors[pendingPublisher.id]!.code" :actions="publishers.errors[pendingPublisher.id]!.actions" />
    </div>
    <details v-if="library.leftovers.length" class="text-sm text-ink-dim">
      <summary class="synra-choice min-h-8 cursor-pointer">
        {{ messages.plugins.leftBehind(library.leftovers.length) }}
      </summary>
      <ul class="m-0 ps-5">
        <li v-for="path in library.leftovers" :key="path" class="break-all font-mono">
          {{ path }}
        </li>
      </ul>
    </details>
    <ErrorNotice v-if="library.error" :code="library.error.code" :actions="library.error.actions" @retry="library.load" />
    <div v-show="category === 'installed'" class="min-w-0 stack gap-3" data-testid="plugin-installed-view">
      <AInput ref="filterInput" v-model:value="filter" class="max-w-md" :aria-label="messages.plugins.filterPlaceholder" :placeholder="messages.plugins.filterPlaceholder" allow-clear />
      <p v-if="library.loading && !store.plugins.length" class="m-0 text-base text-ink-dim" role="status">
        {{ messages.plugins.workflow.loading }}
      </p>
      <AEmpty v-else-if="!store.plugins.length" :description="messages.plugins.empty">
        <AButton @click="installOpen = true">
          {{ messages.plugins.workflow.localInstall }}
        </AButton>
      </AEmpty>
      <AEmpty v-else-if="!visiblePlugins.length" :description="messages.plugins.noMatch">
        <AButton @click="filter = ''">
          {{ messages.plugins.clearFilter }}
        </AButton>
      </AEmpty>
      <div v-else>
        <PluginRow v-for="plugin in visiblePlugins" :key="plugin.pluginId" :plugin="plugin" :library="library" :attention="needsAttention(plugin.publisher)" @details="tab => showDetails(plugin.pluginId, tab, tab === 'permissions' ? 'review' : '')" @action="action => rowAction(plugin, action)" />
      </div>
    </div>
    <RegistryDiscover v-show="category === 'discover'" data-testid="plugin-discover-view" @installed="installed" @details="showDetails" />
    <PluginDetail v-if="targetPlugin" :key="targetPlugin.pluginId" :plugin="targetPlugin" :library="library" :initial-tab="detailTab" :initial-action="detailAction" @close="leaveTarget" />
    <div v-else-if="targetId && !library.loading" class="stack items-start gap-2" data-testid="plugin-target">
      <p class="m-0 text-base">
        {{ messages.plugins.notFound(targetId) }}
      </p><AButton @click="leaveTarget">
        {{ messages.plugins.showAll }}
      </AButton>
    </div>
    <PluginRemoval v-if="removingPlugin" :plugin="removingPlugin" :library="library" @close="removingId = ''" />
    <LocalInstallDialog v-if="installOpen" @close="installOpen = false" @installed="installed" />
    <PublisherManager v-if="publishersOpen" :model="publishers" @close="publishersOpen = false" />
  </section>
</template>

<script setup lang="ts">
import type { InstallReport, PluginSnapshot } from '@synra/protocol'
import type { PluginMenuAction } from '../features/plugins/menu'
import { Button as AButton, Empty as AEmpty, Input as AInput, Segmented as ASegmented } from 'ant-design-vue'
import { computed, defineAsyncComponent, nextTick, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import ErrorNotice from '../components/ErrorNotice.vue'
import PageHeader from '../components/PageHeader.vue'
import PluginRemoval from '../components/plugins/plugin-removal.vue'
import PluginRow from '../components/plugins/plugin-row.vue'
import RegistryDiscover from '../components/plugins/registry-discover.vue'
import { usePluginLibrary } from '../features/plugins/library'
import { usePublishers } from '../features/plugins/publishers'
import { messages } from '../messages'
import { usePluginsStore } from '../stores/plugins'

const PluginDetail = defineAsyncComponent(() => import('../components/plugins/plugin-detail.vue'))
const LocalInstallDialog = defineAsyncComponent(() => import('../components/plugins/local-install-dialog.vue'))
const PublisherManager = defineAsyncComponent(() => import('../components/plugins/publisher-manager.vue'))

const store = usePluginsStore()
const library = usePluginLibrary(onRemoved)
const publishers = usePublishers()
const route = useRoute()
const router = useRouter()
const category = ref('installed')
const filter = ref('')
const installOpen = ref(false)
const publishersOpen = ref(false)
const filterInput = ref<{ focus: () => void }>()
const installToggle = ref<{ $el: HTMLButtonElement }>()
const lastInstalled = ref<InstallReport>()
const pendingPublisher = ref<{ id: string, fingerprint: string }>()
const detailTab = ref('overview')
const detailAction = ref('')
const removingId = ref('')
const removingPlugin = computed(() => store.plugin(removingId.value))
let detailTrigger: HTMLElement | undefined
const categories = computed(() => [{ label: messages.plugins.categories.installed, value: 'installed' }, { label: messages.plugins.categories.discover, value: 'discover' }])
const targetId = computed(() => typeof route.query.plugin === 'string' ? route.query.plugin : '')
const targetPlugin = computed(() => store.plugin(targetId.value))
const visiblePlugins = computed(() => {
  const query = filter.value.trim().toLowerCase()
  return store.plugins.filter(plugin => [plugin.title, plugin.pluginId, plugin.description ?? ''].some(text => text.toLowerCase().includes(query)))
})
const attentionCount = computed(() => publishers.list.filter(publisher => publisher.trust !== 'trusted').length)
function needsAttention(id: string): boolean {
  return publishers.list.some(publisher => publisher.publisherId === id && publisher.trust !== 'trusted')
}
function onRemoved(id: string): void {
  removingId.value = ''
  if (targetId.value === id)
    void leaveTarget()
}
onMounted(() => Promise.all([library.load(), publishers.read()]))

function showDetails(id: string, tab = 'overview', action = ''): void {
  detailAction.value = action
  detailTrigger = document.activeElement instanceof HTMLElement ? document.activeElement : undefined
  detailTab.value = tab
  void router.replace({ path: route.path, query: { ...route.query, plugin: id }, hash: route.hash })
}
async function leaveTarget(): Promise<void> {
  detailAction.value = ''
  const { plugin: _plugin, ...query } = route.query
  await router.replace({ path: route.path, query, hash: route.hash })
  await nextTick()
  if (detailTrigger?.isConnected)
    detailTrigger.focus()
  else
    filterInput.value?.focus()
}
function rowAction(plugin: PluginSnapshot, action: PluginMenuAction): void {
  if (action === 'check')
    void library.checkUpdate(plugin)
  else if (action === 'update')
    void library.update(plugin)
  else if (action === 'remove')
    removingId.value = plugin.pluginId
  else if (action === 'sync')
    showDetails(plugin.pluginId, 'overview', 'sync')
  else
    showDetails(plugin.pluginId, action)
}
async function installed(report: InstallReport): Promise<void> {
  installOpen.value = false
  lastInstalled.value = report
  library.note = messages.plugins.workflow.installedHelp
  library.warnings = report.warnings ?? []
  await Promise.all([library.load(), publishers.read()])
  const known = publishers.list.find(publisher => publisher.publisherId === report.publisherId)
  if (report.publisherId && report.fingerprint && known?.trust !== 'trusted')
    pendingPublisher.value = { id: report.publisherId, fingerprint: report.fingerprint }
  await nextTick()
  installToggle.value?.$el.focus()
}
async function decidePending(trust: 'trusted' | 'revoked'): Promise<void> {
  const pending = pendingPublisher.value
  if (pending && await publishers.decide(pending.id, pending.fingerprint, trust) && pendingPublisher.value === pending)
    pendingPublisher.value = undefined
}
</script>
