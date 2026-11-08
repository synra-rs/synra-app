import type { IpcError } from '@synra/protocol'
import type { Operation } from '../features/settings/writes'
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { onBeforeRouteLeave, useRoute, useRouter } from 'vue-router'
import { kernel } from '../app/kernel'
import { useSettingsDraft } from '../components/settings/use-settings-draft'
import { useSettingsWrites } from '../features/settings/writes'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'
import { useSettingsStore } from '../stores/settings'

export function useSettingsPage() {
  type Category = 'general' | 'security' | 'plugins' | 'diagnostics' | 'about'

  const category = ref<Category>('general')
  const categories = computed(() => (['general', 'security', 'plugins', 'diagnostics', 'about'] as const).map(value => ({ value, label: messages.settings.categories[value] })))
  const route = useRoute()
  function categoryFromQuery(): Category | undefined {
    const asked = route.query.category
    return categories.value.find(item => item.value === asked)?.value
  }
  const categoryHeading = ref<HTMLElement>()
  function setCategoryHeading(element: unknown): void {
    categoryHeading.value = element as HTMLElement | undefined
  }
  const diagnosticsVisited = ref(false)
  const router = useRouter()
  watch(() => route.query.category, () => {
    const asked = categoryFromQuery()
    if (!asked)
      return
    category.value = asked
    if (asked === 'diagnostics')
      diagnosticsVisited.value = true
  }, { immediate: true })
  function selectCategory(value: unknown): void {
    if (!categories.value.some(item => item.value === value))
      return
    category.value = value as Category
    if (route.query.category !== undefined) {
      const { category: _category, ...query } = route.query
      void router.replace({ path: route.path, query, hash: route.hash })
    }
  }
  watch(category, async (value) => {
    if (value === 'diagnostics')
      diagnosticsVisited.value = true
    await nextTick()
    categoryHeading.value?.focus({ preventScroll: true })
  })
  const settings = useSettingsStore()
  const devices = useDevicesStore()
  const identity = computed(() => devices.identity)
  const readError = ref<IpcError>()
  const loading = ref(false)
  let disposed = false
  const { errors, busy, saved, needsRead, anyBusy, configReady, checkSetting, writeSetting } = useSettingsWrites(() => disposed, resetDrafts)
  const { value: nameDraft, dirty: nameDirty, conflict: nameConflict, reset: resetName } = useSettingsDraft(() => identity.value?.displayName ?? '')
  const renaming = ref(false)
  function startRename(): void {
    resetName()
    renaming.value = true
    errors.name = undefined
    saved.name = false
    void nextTick(() => document.getElementById('device-name')?.focus())
  }
  function cancelRename(): void {
    resetName()
    renaming.value = false
    errors.name = undefined
  }
  async function checkName(): Promise<boolean> {
    const answer = await devices.readIdentity(kernel)
    if (disposed)
      return false
    if (!answer.ok) {
      errors.name = answer.error
      return false
    }
    needsRead.name = false
    errors.name = undefined
    saved.name = true
    resetName()
    renaming.value = false
    return true
  }
  async function saveName(): Promise<boolean> {
    if (busy.name)
      return false
    busy.name = true
    errors.name = undefined
    try {
      if (needsRead.name)
        return await checkName()
      if (!nameDirty.value || !nameDraft.value.trim())
        return !nameDirty.value
      const answer = await kernel.call<null>('identity_set_display_name', { name: nameDraft.value })
      if (disposed)
        return false
      if (!answer.ok) {
        errors.name = answer.error
        return false
      }
      needsRead.name = true
      return await checkName()
    }
    finally {
      busy.name = false
    }
  }
  const askCancel = computed(() => settings.value('transfers.confirmCancel') !== false)
  const debugConsole = computed(() => settings.value('ui.debugConsole') !== false)
  const notifyOnReceive = computed(() => settings.value('transfers.notifyOnReceive') !== false)
  const languageOptions = computed(() => [
    { value: 'system', label: messages.settings.language.system },
    { value: 'zh-CN', label: messages.settings.language.chinese },
    { value: 'en', label: messages.settings.language.english }
  ])
  const updatePolicyOptions = computed(() => [
    { value: 'never', label: messages.settings.updates.never },
    { value: 'notify-only', label: messages.settings.updates.notifyOnly },
    { value: 'auto-minor', label: messages.settings.updates.autoMinor },
    { value: 'auto-all', label: messages.settings.updates.autoAll }
  ])
  const updatePolicy = computed(() => {
    const value = settings.value('plugins.auto_update')
    return typeof value === 'string' && updatePolicyOptions.value.some(option => option.value === value) ? value : 'notify-only'
  })
  async function saveLanguage(value: unknown): Promise<void> {
    if (value === 'system' || value === 'en' || value === 'zh-CN')
      await writeSetting('language', { set: { 'ui.locale': value } })
  }
  async function saveUpdatePolicy(value: unknown): Promise<void> {
    if (typeof value === 'string' && updatePolicyOptions.value.some(option => option.value === value))
      await writeSetting('policy', { set: { 'plugins.auto_update': value } })
  }
  function registrySource(): string[] {
    const raw = settings.value('plugins.registry')
    const urls = typeof raw === 'string' ? [raw] : Array.isArray(raw) ? raw.filter((item): item is string => typeof item === 'string') : []
    return urls.length ? urls : ['']
  }
  const { value: registryUrls, dirty: registryDirty, conflict: registryConflict, reset: resetRegistry } = useSettingsDraft(registrySource)
  const { value: retentionDays, dirty: retentionDirty, conflict: retentionConflict, reset: resetRetention } = useSettingsDraft(() => {
    const raw = settings.value('audit.retentionDays')
    return typeof raw === 'number' && Number.isFinite(raw) ? raw : 7
  })
  function updateRegistry(index: number, value: string): void {
    registryUrls.value = registryUrls.value.map((url, at) => index === at ? value : url)
    saved.registry = false
  }
  function resetDrafts(operation: Operation): void {
    if (operation === 'registry')
      resetRegistry()
    if (operation === 'retention')
      resetRetention()
  }
  async function saveRegistry(): Promise<boolean> {
    if (!registryDirty.value && !needsRead.registry)
      return true
    const urls = registryUrls.value.map(url => url.trim()).filter(Boolean)
    return await writeSetting('registry', urls.length ? { set: { 'plugins.registry': urls.length === 1 ? urls[0]! : urls } } : { unset: ['plugins.registry'] })
  }
  async function saveRetention(): Promise<boolean> {
    if (!retentionDirty.value && !needsRead.retention)
      return true
    return await writeSetting('retention', { set: { 'audit.retentionDays': Math.min(3650, Math.max(1, Math.trunc(retentionDays.value))) } })
  }
  const trusted = computed(() => {
    const raw = settings.value('transfers.acceptFrom')
    return (Array.isArray(raw) ? raw.filter((id): id is string => typeof id === 'string') : []).map(id => ({ id, name: devices.get(id)?.displayName ?? id }))
  })
  const removingId = ref<string>()
  async function forgetTrusted(id: string): Promise<void> {
    if (busy.trusted || needsRead.trusted)
      return
    removingId.value = id
    const remaining = trusted.value.filter(device => device.id !== id).map(device => device.id)
    await writeSetting('trusted', remaining.length ? { set: { 'transfers.acceptFrom': remaining } } : { unset: ['transfers.acceptFrom'] })
  }
  const launchAtLogin = ref(false)
  const launchAtLoginUnavailable = ref(false)
  async function loadLaunchAtLogin(): Promise<void> {
    try {
      const { isEnabled } = await import('@tauri-apps/plugin-autostart')
      launchAtLogin.value = await isEnabled()
    }
    catch {
      launchAtLoginUnavailable.value = true
    }
  }
  async function saveLaunchAtLogin(value: boolean): Promise<void> {
    if (busy.startup || launchAtLoginUnavailable.value)
      return
    busy.startup = true
    errors.startup = undefined
    saved.startup = false
    try {
      const { enable, disable, isEnabled } = await import('@tauri-apps/plugin-autostart')
      if (value)
        await enable()
      else
        await disable()
      launchAtLogin.value = await isEnabled()
      saved.startup = true
    }
    catch {
      errors.startup = { code: 'internal', message: 'error.internal', retryable: false }
    }
    finally {
      busy.startup = false
    }
  }
  const pluginDrafts = computed(() => registryDirty.value || retentionDirty.value || needsRead.registry || needsRead.retention)
  const hasDrafts = computed(() => (renaming.value && (nameDirty.value || needsRead.name)) || pluginDrafts.value)
  const leaveOpen = ref(false)
  let leaveResolve: ((leave: boolean) => void) | undefined
  function resolveLeave(leave: boolean): void {
    leaveOpen.value = false
    leaveResolve?.(leave)
    leaveResolve = undefined
  }
  onBeforeRouteLeave(() => {
    if (!hasDrafts.value && !anyBusy.value)
      return true
    if (leaveResolve)
      return false
    leaveOpen.value = true
    return new Promise<boolean>((resolve) => {
      leaveResolve = resolve
    })
  })
  async function saveAndLeave(): Promise<void> {
    if (anyBusy.value)
      return
    if (nameConflict.value || registryConflict.value || retentionConflict.value) {
      category.value = nameConflict.value ? 'general' : 'plugins'
      resolveLeave(false)
      return
    }
    if (renaming.value && !await saveName()) {
      category.value = 'general'
      resolveLeave(false)
      return
    }
    if (!await saveRegistry() || !await saveRetention()) {
      category.value = 'plugins'
      resolveLeave(false)
      return
    }
    resolveLeave(true)
  }
  function beforeUnload(event: BeforeUnloadEvent): void {
    if (hasDrafts.value || anyBusy.value) {
      event.preventDefault()
      event.returnValue = ''
    }
  }
  async function load(): Promise<void> {
    if (loading.value)
      return
    loading.value = true
    const [config, name] = await Promise.all([settings.read(kernel), devices.readIdentity(kernel)])
    if (!disposed) {
      if (config.ok)
        configReady.value = true
      readError.value = !config.ok ? config.error : !name.ok ? name.error : undefined
    }
    loading.value = false
  }
  onMounted(() => {
    void load()
    void loadLaunchAtLogin()
    window.addEventListener('beforeunload', beforeUnload)
  })
  onUnmounted(() => {
    disposed = true
    resolveLeave(false)
    window.removeEventListener('beforeunload', beforeUnload)
  })
  return { category, categories, setCategoryHeading, diagnosticsVisited, selectCategory, identity, readError, loading, renaming, startRename, cancelRename, checkName, saveName, askCancel, debugConsole, notifyOnReceive, languageOptions, updatePolicyOptions, updatePolicy, saveLanguage, saveUpdatePolicy, updateRegistry, saveRegistry, saveRetention, trusted, removingId, forgetTrusted, launchAtLogin, launchAtLoginUnavailable, saveLaunchAtLogin, pluginDrafts, hasDrafts, resolveLeave, saveAndLeave, load, errors, busy, saved, needsRead, anyBusy, configReady, checkSetting, writeSetting, nameDraft, nameDirty, nameConflict, registryUrls, registryDirty, registryConflict, retentionDays, retentionDirty, retentionConflict, resetName, resetRegistry, resetRetention, leaveOpen }
}
