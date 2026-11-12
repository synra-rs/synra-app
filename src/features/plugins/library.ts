import type { InstallReport, PluginSnapshot, UpdateInfo } from '@synra/protocol'
import type { Failure } from './actions'
import { onScopeDispose, reactive, ref } from 'vue'
import { kernel } from '../../app/kernel'
import { messages } from '../../messages'
import { usePluginsStore } from '../../stores/plugins'
import { formatBytes } from '../../utils/format'
import { failureOf, setEnabled, uninstall } from './actions'
import { usePluginOperations } from './operations'

export function usePluginLibrary(onRemoved?: (id: string) => void) {
  const store = usePluginsStore()
  const operations = usePluginOperations()
  const errors = reactive<Record<string, Failure | undefined>>({})
  const error = ref<Failure>()
  const loading = ref(false)
  const note = ref('')
  const warnings = ref<readonly string[]>([])
  const leftovers = ref<readonly string[]>([])
  let alive = true
  onScopeDispose(() => alive = false)

  async function load(): Promise<void> {
    if (loading.value)
      return
    loading.value = true
    error.value = undefined
    const before = store.plugins
    try {
      const answer = await kernel.call<readonly PluginSnapshot[]>('plugins_list')
      if (!alive)
        return
      if (!answer.ok)
        error.value = failureOf(answer.error)
      else if (store.plugins === before)
        store.applySnapshot(answer.value)
    }
    finally {
      loading.value = false
    }
  }

  async function toggle(plugin: PluginSnapshot, enabled: boolean): Promise<void> {
    delete errors[plugin.pluginId]
    const answer = await operations.run(plugin.pluginId, 'toggle', () => setEnabled(kernel, plugin.pluginId, enabled))
    if (answer && !answer.ok)
      errors[plugin.pluginId] = answer.failure
  }

  async function open(plugin: PluginSnapshot): Promise<void> {
    delete errors[plugin.pluginId]
    const answer = await operations.run(plugin.pluginId, 'open', () => kernel.call<null>('plugin_ui_open', { pluginId: plugin.pluginId }))
    if (answer && !answer.ok)
      errors[plugin.pluginId] = failureOf(answer.error)
  }

  async function retry(plugin: PluginSnapshot): Promise<void> {
    delete errors[plugin.pluginId]
    const answer = await operations.run(plugin.pluginId, 'reload', () => kernel.call<PluginSnapshot>('plugins_reload', { pluginId: plugin.pluginId }))
    if (!answer)
      return
    if (answer.ok)
      store.upsert(answer.value)
    else
      errors[plugin.pluginId] = failureOf(answer.error)
  }

  async function checkUpdate(plugin: PluginSnapshot): Promise<void> {
    delete errors[plugin.pluginId]
    const answer = await operations.run(plugin.pluginId, 'check', () => kernel.call<UpdateInfo | null>('plugins_check_update', { pluginId: plugin.pluginId }))
    if (!answer)
      return
    if (!answer.ok) {
      errors[plugin.pluginId] = failureOf(answer.error)
      return
    }
    const current = store.plugin(plugin.pluginId)
    if (current) {
      const { update: _update, ...rest } = current
      store.upsert(answer.value ? { ...rest, update: answer.value } : rest)
    }
  }

  async function update(plugin: PluginSnapshot): Promise<void> {
    delete errors[plugin.pluginId]
    const answer = await operations.run(plugin.pluginId, 'update', () => kernel.call<InstallReport>('plugins_update', { pluginId: plugin.pluginId }))
    if (!answer)
      return
    if (!answer.ok) {
      errors[plugin.pluginId] = failureOf(answer.error)
      return
    }
    note.value = messages.plugins.updatedTo.replace('{pluginId}', answer.value.pluginId).replace('{version}', answer.value.version)
    warnings.value = answer.value.warnings ?? []
    await load()
  }

  async function reloadLocal(plugin: PluginSnapshot, removeData: boolean): Promise<boolean> {
    delete errors[plugin.pluginId]
    const answer = await operations.run(plugin.pluginId, 'reload', () => kernel.call<InstallReport>('plugins_update', { pluginId: plugin.pluginId, removeData }))
    if (!answer)
      return false
    if (!answer.ok) {
      errors[plugin.pluginId] = failureOf(answer.error)
      return false
    }
    note.value = messages.plugins.workflow.reloadAction
    warnings.value = answer.value.warnings ?? []
    await load()
    return true
  }

  async function remove(plugin: PluginSnapshot, removeData: boolean): Promise<boolean> {
    delete errors[plugin.pluginId]
    const answer = await operations.run(plugin.pluginId, 'remove', () => uninstall(kernel, plugin.pluginId, removeData))
    if (!answer)
      return false
    if (!answer.ok) {
      errors[plugin.pluginId] = answer.failure
      return false
    }
    store.forget(plugin.pluginId)
    onRemoved?.(plugin.pluginId)
    leftovers.value = answer.value.leftovers ?? []
    const freed = formatBytes(answer.value.bytesFreed)
    note.value = answer.value.dataKept ? messages.plugins.removedKept(plugin.title, freed) : messages.plugins.removedDeleted(plugin.title, freed)
    await load()
    return true
  }

  return reactive({ operations, errors, error, loading, note, warnings, leftovers, load, toggle, open, retry, checkUpdate, update, reloadLocal, remove })
}

export type PluginLibrary = ReturnType<typeof usePluginLibrary>
