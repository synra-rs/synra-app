import type { Operation, useSettingsWrites } from './writes'
import { computed } from 'vue'
import { useSettingsDraft } from '../../components/settings/use-settings-draft'
import { messages } from '../../messages'
import { useSettingsStore } from '../../stores/settings'

type SettingsWrites = Pick<ReturnType<typeof useSettingsWrites>, 'saved' | 'needsRead' | 'writeSetting'>

export function usePluginPreferences({ saved, needsRead, writeSetting }: SettingsWrites) {
  const settings = useSettingsStore()
  const updatePolicyOptions = computed(() => [
    { value: 'never', label: messages.settings.updates.never },
    { value: 'notify-only', label: messages.settings.updates.notifyOnly },
    { value: 'auto-minor', label: messages.settings.updates.autoMinor },
    { value: 'auto-all', label: messages.settings.updates.autoAll }
  ])
  const updatePolicy = computed(() => {
    const value = settings.value('plugins.auto_update')
    return typeof value === 'string' && updatePolicyOptions.value.some(option => option.value === value)
      ? value
      : 'notify-only'
  })
  async function saveUpdatePolicy(value: unknown): Promise<void> {
    if (typeof value === 'string' && updatePolicyOptions.value.some(option => option.value === value)) {
      await writeSetting('policy', {
        set: { 'plugins.auto_update': value }
      })
    }
  }
  function registrySource(): string[] {
    const raw = settings.value('plugins.registry')
    const display = (item: unknown): string => typeof item === 'string'
      ? item
      : JSON.stringify(item) ?? ''
    const urls = raw === undefined
      ? []
      : Array.isArray(raw)
        ? raw.map(display)
        : [
            display(raw)
          ]
    return urls.length
      ? urls
      : ['']
  }
  const { value: registryUrls, dirty: registryDirty, conflict: registryConflict, reset: resetRegistry } = useSettingsDraft(registrySource)
  const { value: retentionDays, dirty: retentionDirty, conflict: retentionConflict, reset: resetRetention } = useSettingsDraft(() => {
    const raw = settings.value('audit.retentionDays')
    return typeof raw === 'number' && Number.isFinite(raw)
      ? raw
      : 7
  })
  function updateRegistry(index: number, value: string): void {
    registryUrls.value = registryUrls.value.map((url, at) => index === at
      ? value
      : url)
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
    const urls = registryUrls.value.map(url => url.trim())
    const empty = urls.length === 0 || (urls.length === 1 && urls[0] === '')
    return await writeSetting('registry', empty
      ? {
          unset: ['plugins.registry']
        }
      : {
          set: {
            'plugins.registry': urls.length === 1
              ? urls[0]!
              : urls
          }
        })
  }
  async function saveRetention(): Promise<boolean> {
    if (!retentionDirty.value && !needsRead.retention)
      return true
    return await writeSetting('retention', {
      set: {
        'audit.retentionDays': Math.min(3650, Math.max(1, Math.trunc(retentionDays.value)))
      }
    })
  }
  const pluginDrafts = computed(() => registryDirty.value || retentionDirty.value || needsRead.registry || needsRead.retention)

  return {
    updatePolicyOptions,
    updatePolicy,
    saveUpdatePolicy,
    updateRegistry,
    saveRegistry,
    saveRetention,
    pluginDrafts,
    registryUrls,
    registryDirty,
    registryConflict,
    retentionDays,
    retentionDirty,
    retentionConflict,
    resetRegistry,
    resetRetention,
    resetDrafts
  }
}
