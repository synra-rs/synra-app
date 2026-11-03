import type { Ref } from 'vue'
import { defineStore } from 'pinia'
import { computed, onScopeDispose, reactive, watch } from 'vue'

const useDialogsStore = defineStore('dialogs', () => {
  const requests = reactive(new Map<symbol, number>())
  const active = computed(() => [...requests].sort((a, b) => b[1] - a[1])[0]?.[0])
  return { requests, active }
})

/** Suspend presentation, keeping the owning workflow and its draft mounted. */
export function useDialogVisibility(requested: Ref<boolean>, priority = 0) {
  const dialogs = useDialogsStore()
  const id = Symbol('dialog')
  watch(requested, (open) => {
    if (open)
      dialogs.requests.set(id, priority)
    else
      dialogs.requests.delete(id)
  }, { immediate: true, flush: 'sync' })
  onScopeDispose(() => dialogs.requests.delete(id))
  return computed(() => requested.value && dialogs.active === id)
}
