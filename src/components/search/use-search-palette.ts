import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'

import { PAGES } from '../../app/router'
import { messages } from '../../messages'
import { useDevicesStore } from '../../stores/devices'
import { usePluginsStore } from '../../stores/plugins'
import { guardCompositionEscape, isComposing } from '../../utils/keyboard'

export function useSearchPalette() {
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
  const field = ref<{ focus: () => void }>()
  function setField(value: unknown): void {
    field.value = value as { focus: () => void } | undefined
  }
  const activeId = ref<string>()

  const SETTINGS_SECTIONS = ['general', 'security', 'plugins', 'diagnostics', 'about'] as const

  function optionDomId(id: string): string {
    return `search-option-${encodeURIComponent(id)}`
  }

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

      to: `/devices?device=${encodeURIComponent(card.deviceId)}`
    })),
    ...PAGES.map(page => ({
      id: `page:${page.path}`,
      label: page.label,
      icon: page.icon,
      group: 'pages' as const,
      to: page.path
    }))
  ])

  const results = computed<readonly Item[]>(() => {
    const needle = query.value.trim().toLowerCase()
    if (needle === '')
      return items.value.filter(item => item.group === 'pages' || item.group === 'settings')
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

  function onKeydown(event: KeyboardEvent): void {
    guardCompositionEscape(event)
    if (isComposing(event))
      return
    if (event.key === 'Escape') {
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

  function isTyping(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement))
      return false
    if (target.isContentEditable)
      return true
    const tag = target.tagName
    return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT'
  }

  function onGlobalKeydown(event: KeyboardEvent): void {
    const commandK = (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k'
    const slash = event.key === '/' && !event.metaKey && !event.ctrlKey && !event.altKey
    if (!commandK && !slash)
      return

    if (isComposing(event))
      return
    if (!commandK && isTyping(event.target))
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

  return { open, query, setField, activeId, results, grouped, optionDomId, show, close, choose, onKeydown }
}
