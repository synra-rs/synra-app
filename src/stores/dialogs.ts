import type { Ref } from 'vue'
import { defineStore } from 'pinia'
import { computed, onScopeDispose, reactive, watch } from 'vue'

interface DialogRequest {
  priority: number
  dismiss?: () => void
}

const useDialogsStore = defineStore('dialogs', () => {
  const requests = reactive(new Map<symbol, DialogRequest>())
  const activeRequest = computed(() => [...requests].sort((a, b) => b[1].priority - a[1].priority)[0])
  const active = computed(() => activeRequest.value?.[0])

  function dismissActive(): boolean {
    const request = activeRequest.value?.[1]
    if (!request)
      return false
    request.dismiss?.()
    return true
  }

  return { requests, active, dismissActive }
})

/** Suspend presentation, keeping the owning workflow and its draft mounted. */
export function useDialogVisibility(requested: Ref<boolean>, priority = 0, dismiss?: () => void) {
  const dialogs = useDialogsStore()
  const id = Symbol('dialog')
  watch(requested, (open) => {
    if (!open) {
      dialogs.requests.delete(id)
      return
    }
    const request: DialogRequest = { priority }
    if (dismiss)
      request.dismiss = dismiss
    dialogs.requests.set(id, request)
  }, { immediate: true, flush: 'sync' })
  onScopeDispose(() => dialogs.requests.delete(id))
  return computed(() => requested.value && dialogs.active === id)
}

/** Handle the mobile back action at the highest visible dialog before leaving the page or app. */
export function dismissActiveDialog(): boolean {
  return useDialogsStore().dismissActive()
}
