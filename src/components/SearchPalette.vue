<template>
  <!--
    The shell's one search box.

    ★ **Why a modal rather than a page.** A search page would be a fifth destination for a tool that exists to avoid
    navigating; the whole point is to reach a plugin, a settings section or a device without first going somewhere to
    look for it. It is anchored to the shell, so the shortcut works from every page.

    ★ **Why it reads stores rather than the kernel.** Everything it can find is already in memory: `plugins` holds the
    installed list, `devices` the cards, and the settings sections and pages are constants. Reading them here is what
    keeps this from needing a new command, and F's boundary says not to add one
    ([10](../../../design-docs/ui-design/10-delivery-and-acceptance.md) §2).
  -->
  <AModal
    :open="open"
    :footer="null"
    :closable="false"
    :width="560"
    :destroy-on-close="false"
    wrap-class-name="search-modal"
    @cancel="close"
  >
    <div class="stack gap-3">
      <div class="row items-center gap-2 border border-line rounded-control bg-raised px-3">
        <span class="i-carbon-search shrink-0 text-ink-dim" aria-hidden="true" />
        <input
          ref="field"
          v-model="query"
          type="text"
          class="min-w-0 w-full bg-transparent py-2 text-sm outline-none"
          :placeholder="messages.search.placeholder"
          :aria-label="messages.search.placeholder"
          role="combobox"
          aria-controls="search-results"
          :aria-expanded="results.length > 0"
          @keydown="onKeydown"
        >
      </div>

      <!--
        ★ The groups are named because the list mixes four kinds of object. A flat list of "Diagnostics, Starter, Pixel 8"
        leaves the reader working out what each row is; the heading says it once for the group.
      -->
      <div v-if="results.length > 0" id="search-results" class="max-h-80 overflow-y-auto overscroll-contain" role="listbox" :aria-label="messages.search.title">
        <template v-for="group in grouped" :key="group.id">
          <div class="mb-1 mt-1 text-xs text-ink-dim">
            {{ group.label }}
          </div>
          <!--
            ★ Hover is `surface`, selection is `raised`. They used to be the same token, so a pointer resting on one row
            while the arrow keys moved the selection to another painted two rows identically — and the reader could not
            tell which one Enter would run (`09 §4`).
          -->
          <button
            v-for="item in group.items"
            :key="item.id"
            type="button"
            role="option"
            :aria-selected="item.id === activeId"
            class="min-h-11 w-full row items-center gap-2 rounded-control px-2 text-start text-sm hover:bg-surface"
            :class="{ 'bg-raised': item.id === activeId }"
            @click="choose(item)"
            @mouseenter="activeId = item.id"
          >
            <span :class="item.icon" class="shrink-0 text-ink-dim" aria-hidden="true" />
            <span class="min-w-0 flex-1 truncate">{{ item.label }}</span>
            <span v-if="item.detail" class="shrink-0 text-xs text-ink-dim">{{ item.detail }}</span>
          </button>
        </template>
      </div>

      <p v-else-if="query.trim() !== ''" class="m-0 text-sm text-ink-dim">
        {{ messages.search.empty }}
      </p>
      <!-- What is searched, said once, and only while the box is empty. -->
      <p v-else class="m-0 text-sm text-ink-dim">
        {{ messages.search.scope }}
      </p>
    </div>
  </AModal>
</template>

<script setup lang="ts">
/**
 * What the shell can find, and what choosing a result does.
 *
 * The component owns its own open state and the `/` listener, because the entry and the shortcut must agree about what
 * "search is open" means. Everything it searches is read from the stores on the way in, so a plugin installed while
 * the box is closed is found on the next open without any subscription of its own.
 */
import { Modal as AModal } from 'ant-design-vue'
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'

import { messages } from '../messages'
import { PAGES } from '../router'
import { useDevicesStore } from '../stores/devices'
import { usePluginsStore } from '../stores/plugins'

interface Item {
  id: string
  label: string
  detail?: string
  icon: string
  group: 'plugins' | 'settings' | 'devices' | 'pages'
  to: string
}

const router = useRouter()
const plugins = usePluginsStore()
const devices = useDevicesStore()

const open = ref(false)
const query = ref('')
const field = ref<HTMLInputElement>()
const activeId = ref<string>()

/** The settings sections, by the routes they are reachable at. */
const SETTINGS_SECTIONS = ['general', 'security', 'plugins', 'diagnostics', 'about'] as const

const items = computed<readonly Item[]>(() => [
  ...plugins.plugins.map(plugin => ({
    id: `plugin:${plugin.pluginId}`,
    label: plugin.title,
    detail: plugin.pluginId,
    icon: 'i-carbon-application',
    group: 'plugins' as const,
    to: `/plugins?plugin=${encodeURIComponent(plugin.pluginId)}`
  })),
  ...SETTINGS_SECTIONS.map(section => ({
    id: `settings:${section}`,
    label: messages.settings.categories[section],
    icon: 'i-carbon-settings',
    group: 'settings' as const,
    to: `/settings?category=${section}`
  })),
  ...devices.cards.map(card => ({
    id: `device:${card.deviceId}`,
    label: card.displayName,
    detail: card.platform,
    icon: 'i-carbon-devices',
    group: 'devices' as const,
    to: '/devices'
  })),
  ...PAGES.map(page => ({
    id: `page:${page.path}`,
    label: page.label,
    icon: page.icon,
    group: 'pages' as const,
    to: page.path
  }))
])

/**
 * A substring match over the label, the detail and — for plugins — the id, case-insensitively. No index is built: the
 * lists are tens of entries at most, and a prebuilt index is the expensive thing F's boundary forbids.
 */
const results = computed<readonly Item[]>(() => {
  const needle = query.value.trim().toLowerCase()
  if (needle === '')
    return []
  return items.value.filter(item =>
    item.label.toLowerCase().includes(needle)
    || (item.detail ?? '').toLowerCase().includes(needle)
  )
})

const GROUP_LABELS = computed(() => ({
  plugins: messages.search.groups.plugins,
  settings: messages.search.groups.settings,
  devices: messages.search.groups.devices,
  pages: messages.search.groups.pages
}))

const grouped = computed(() => {
  const order = ['plugins', 'settings', 'devices', 'pages'] as const
  return order
    .map(id => ({ id, label: GROUP_LABELS.value[id], items: results.value.filter(item => item.group === id) }))
    .filter(group => group.items.length > 0)
})

// The top result is where Enter goes, and typing moves it back to the top rather than leaving it on a row that is no
// longer first.
watch(results, (value) => {
  activeId.value = value[0]?.id
})

function show(): void {
  open.value = true
  query.value = ''
  void nextTick(() => field.value?.focus())
}

function close(): void {
  open.value = false
}

function choose(item: Item): void {
  close()
  void router.push(item.to)
}

/**
 * ↑ ↓ move the selection, Enter takes it, Escape closes.
 *
 * ★ **`/` is not handled here.** It opens the box, and a slash typed inside it is a slash — a shortcut that also fired
 *   in the field would make `/` untypable in the one place a person is most likely to need it.
 */
function onKeydown(event: KeyboardEvent): void {
  if (event.key === 'Escape') {
    // Leaving the input's own Escape alone would close the modal anyway; this keeps the focus order predictable.
    event.preventDefault()
    close()
    return
  }
  if (event.key === 'Enter') {
    event.preventDefault()
    const chosen = results.value.find(item => item.id === activeId.value)
    if (chosen)
      choose(chosen)
    return
  }
  if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp')
    return
  event.preventDefault()
  const list = results.value
  if (list.length === 0)
    return
  const at = list.findIndex(item => item.id === activeId.value)
  const next = event.key === 'ArrowDown'
    ? (at + 1) % list.length
    : (at - 1 + list.length) % list.length
  activeId.value = list[next]?.id
}

/**
 * Opens on `/`, except where a slash belongs to the text: any editable target (a field, a textarea, a contenteditable)
 * and any open antd dialog keep it. Without the first check `/` could not be typed into the plugin filter; without the
 * second, opening search on top of a confirm dialog would let a keystroke dismiss a decision.
 */
function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement))
    return false
  if (target.isContentEditable)
    return true
  const tag = target.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT'
}

function onGlobalKeydown(event: KeyboardEvent): void {
  if (event.key !== '/')
    return
  if (event.metaKey || event.ctrlKey || event.altKey)
    return
  // An input method mid-composition sends its own keys; taking `/` there would break other layouts.
  if (event.isComposing)
    return
  if (isTyping(event.target))
    return
  if (document.querySelector('.ant-modal-root:not([style*="display: none"]) .ant-modal-wrap:not([style*="display: none"])') && !open.value)
    return
  event.preventDefault()
  if (open.value)
    void nextTick(() => field.value?.focus())
  else
    show()
}

onMounted(() => window.addEventListener('keydown', onGlobalKeydown))
onUnmounted(() => window.removeEventListener('keydown', onGlobalKeydown))

defineExpose({ show })
</script>
