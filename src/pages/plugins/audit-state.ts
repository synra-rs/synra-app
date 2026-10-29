import type { AuditEntryDto } from '@synra/protocol'
import type { Ref } from 'vue'
import type { KernelClient } from '../../ipc/client'
import type { Failure } from './actions'
import { computed, onScopeDispose, ref, watch } from 'vue'
import { audit, AUDIT_PAGE, clearAudit } from './actions'

/** One visible audit view owns its reads; a closed view cannot publish an older page into a reopened one. */
export function usePluginAudit(client: KernelClient, pluginId: Ref<string>, active: Ref<boolean>) {
  const expanded = ref(false)
  const rows = ref<AuditEntryDto[]>()
  const done = ref(false)
  const error = ref<Failure>()
  const busy = ref<'first' | 'more' | 'clear'>()
  let revision = 0
  let alive = true
  let refreshQueued = false
  const retryOperation = ref<'first' | 'more' | 'clear'>('first')

  function close(): void {
    revision++
    expanded.value = false
    refreshQueued = false
  }

  watch([pluginId, active], ([id, enabled], [previousId]) => {
    if (id !== previousId || !enabled) {
      close()
      rows.value = undefined
      done.value = false
      error.value = undefined
    }
  }, { flush: 'sync' })

  onScopeDispose(() => {
    alive = false
    close()
  })

  function finish(): void {
    busy.value = undefined
    if (alive && refreshQueued && expanded.value && active.value) {
      refreshQueued = false
      void read(false)
    }
  }

  async function read(more: boolean): Promise<void> {
    if (!alive || !active.value || !expanded.value || busy.value || (more && (done.value || !rows.value?.length)))
      return
    const id = pluginId.value
    const current = revision
    const offset = more ? rows.value?.length ?? 0 : 0
    busy.value = more ? 'more' : 'first'
    retryOperation.value = more ? 'more' : 'first'
    error.value = undefined
    const outcome = await audit(client, id, AUDIT_PAGE, offset)
    if (alive && revision === current && expanded.value && active.value) {
      if (outcome.ok) {
        rows.value = more ? [...(rows.value ?? []), ...outcome.value] : [...outcome.value]
        done.value = outcome.value.length < AUDIT_PAGE
      }
      else {
        error.value = outcome.failure
      }
    }
    finish()
  }

  async function setExpanded(open: boolean): Promise<void> {
    if (!open || !active.value) {
      close()
      return
    }
    if (expanded.value)
      return
    revision++
    expanded.value = true
    rows.value = undefined
    done.value = false
    error.value = undefined
    if (busy.value) {
      refreshQueued = true
      return
    }
    await read(false)
  }

  async function clear(): Promise<boolean> {
    if (!alive || !active.value || !expanded.value || busy.value || !rows.value?.length)
      return false
    const current = ++revision
    busy.value = 'clear'
    retryOperation.value = 'clear'
    error.value = undefined
    const outcome = await clearAudit(client, pluginId.value)
    const relevant = alive && current === revision && expanded.value && active.value
    if (relevant) {
      if (outcome.ok) {
        rows.value = []
        done.value = true
      }
      else {
        error.value = outcome.failure
      }
    }
    finish()
    return relevant && outcome.ok
  }

  return {
    expanded,
    rows,
    done,
    error,
    busy,
    loading: computed(() => busy.value === 'first' || busy.value === 'more'),
    clearing: computed(() => busy.value === 'clear'),
    setExpanded,
    retryOperation: computed(() => retryOperation.value),
    loadMore: () => read(true),
    retry: () => retryOperation.value === 'clear' ? clear() : read(retryOperation.value === 'more'),
    clear
  }
}
