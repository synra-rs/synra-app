import type { ConfigPatch, IpcError } from '@synra/protocol'
import { computed, reactive, ref } from 'vue'
import { kernel } from '../../kernel'
import { useSettingsStore } from '../../stores/settings'

export type Operation = 'name' | 'language' | 'startup' | 'wire' | 'cancel' | 'notify' | 'trusted' | 'registry' | 'policy' | 'retention'

export function useSettingsWrites(isDisposed: () => boolean, reset: (operation: Operation) => void) {
  const settings = useSettingsStore()
  const errors = reactive<Partial<Record<Operation, IpcError | undefined>>>({})
  const operationFlags = (): Record<Operation, boolean> => ({ name: false, language: false, startup: false, wire: false, cancel: false, notify: false, trusted: false, registry: false, policy: false, retention: false })
  const busy = reactive(operationFlags())
  const saved = reactive(operationFlags())
  const needsRead = reactive(operationFlags())
  const anyBusy = computed(() => Object.values(busy).some(Boolean))
  const configReady = ref(false)
  async function readSetting(operation: Operation): Promise<boolean> {
    const answer = await settings.read(kernel)
    if (isDisposed())
      return false
    if (!answer.ok) {
      errors[operation] = answer.error
      return false
    }
    configReady.value = true
    needsRead[operation] = false
    errors[operation] = undefined
    saved[operation] = true
    reset(operation)
    return true
  }
  async function checkSetting(operation: Operation): Promise<boolean> {
    if (busy[operation])
      return false
    busy[operation] = true
    try {
      return await readSetting(operation)
    }
    finally {
      busy[operation] = false
    }
  }
  async function writeSetting(operation: Operation, patch: ConfigPatch): Promise<boolean> {
    if (busy[operation] || !configReady.value)
      return false
    busy[operation] = true
    saved[operation] = false
    errors[operation] = undefined
    try {
      if (needsRead[operation])
        return await readSetting(operation)
      const answer = await kernel.call<null>('config_set', { patch })
      if (isDisposed())
        return false
      if (!answer.ok) {
        errors[operation] = answer.error
        return false
      }
      needsRead[operation] = true
      return await readSetting(operation)
    }
    finally {
      busy[operation] = false
    }
  }
  return { errors, busy, saved, needsRead, anyBusy, configReady, checkSetting, writeSetting }
}
