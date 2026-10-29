import type { AuditEntryDto } from '@synra/protocol'
import { describe, expect, it, vi } from 'vitest'
import { effectScope, ref } from 'vue'
import { KernelClient } from '../../ipc/client'
import { usePluginAudit } from './audit-state'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function entries(count: number, capability = 'log:*'): AuditEntryDto[] {
  return Array.from({ length: count }, (_, atMs) => ({ atMs, capability, allowed: true }))
}

function setup(invoke: (command: string, args?: unknown) => Promise<unknown>) {
  const scope = effectScope()
  const id = ref('@acme/one')
  const active = ref(true)
  const client = new KernelClient({ invoke, listen: async () => () => {} })
  const audit = scope.run(() => usePluginAudit(client, id, active))!
  return { scope, id, active, audit }
}

describe('audit view ownership', () => {
  it('queues at most one fresh read after rapid close and reopen while discarding the old page', async () => {
    const first = deferred<AuditEntryDto[]>()
    const second = deferred<AuditEntryDto[]>()
    const invoke = vi.fn().mockImplementationOnce(() => first.promise).mockImplementationOnce(() => second.promise)
    const { scope, audit } = setup(invoke)
    try {
      expect(invoke).not.toHaveBeenCalled()
      const opening = audit.setExpanded(true)
      await audit.setExpanded(false)
      await audit.setExpanded(true)
      await audit.setExpanded(false)
      await audit.setExpanded(true)
      expect(invoke).toHaveBeenCalledTimes(1)
      first.resolve(entries(20, 'stale'))
      await opening
      expect(invoke).toHaveBeenCalledTimes(2)
      expect(audit.rows.value).toBeUndefined()
      second.resolve(entries(1, 'current'))
      await vi.waitFor(() => expect(audit.rows.value?.[0]?.capability).toBe('current'))
      expect(audit.done.value).toBe(true)
    }
    finally {
      scope.stop()
    }
  })

  it('does not apply a former plugin response after an object change', async () => {
    const pending = deferred<AuditEntryDto[]>()
    const invoke = vi.fn().mockImplementationOnce(() => pending.promise).mockResolvedValueOnce(entries(1, 'new-plugin'))
    const { scope, id, audit } = setup(invoke)
    try {
      const opening = audit.setExpanded(true)
      id.value = '@acme/two'
      expect(audit.expanded.value).toBe(false)
      await audit.setExpanded(true)
      pending.resolve(entries(20, 'old-plugin'))
      await opening
      await vi.waitFor(() => expect(audit.rows.value?.[0]?.capability).toBe('new-plugin'))
      expect(invoke).toHaveBeenLastCalledWith('plugins_audit', { pluginId: '@acme/two', limit: 20, offset: 0 })
    }
    finally {
      scope.stop()
    }
  })

  it('keeps rows on a pagination refusal and prevents duplicate reads and clearing during that read', async () => {
    const pending = deferred<unknown>()
    const invoke = vi.fn().mockResolvedValueOnce(entries(20)).mockImplementationOnce(() => pending.promise).mockResolvedValueOnce(entries(1, 'next'))
    const { scope, audit } = setup(invoke)
    try {
      await audit.setExpanded(true)
      const more = audit.loadMore()
      await audit.loadMore()
      expect(await audit.clear()).toBe(false)
      expect(invoke).toHaveBeenCalledTimes(2)
      pending.resolve({ code: 'unavailable', message: 'error.unavailable', retryable: true })
      await more
      expect(audit.rows.value).toHaveLength(20)
      expect(audit.error.value?.code).toBe('unavailable')
      await audit.retry()
      expect(invoke).toHaveBeenLastCalledWith('plugins_audit', { pluginId: '@acme/one', limit: 20, offset: 20 })
      expect(audit.rows.value).toHaveLength(21)
      expect(audit.error.value).toBeUndefined()
    }
    finally {
      scope.stop()
    }
  })

  it('retains records after a failed clear and empties them only after a successful retry', async () => {
    const invoke = vi.fn().mockResolvedValueOnce(entries(1)).mockRejectedValueOnce({ code: 'unavailable', message: 'error.unavailable', retryable: true }).mockResolvedValueOnce(null)
    const { scope, audit } = setup(invoke)
    try {
      await audit.setExpanded(true)
      expect(await audit.clear()).toBe(false)
      expect(audit.rows.value).toHaveLength(1)
      expect(audit.error.value?.code).toBe('unavailable')
      expect(await audit.retry()).toBe(true)
      expect(audit.rows.value).toEqual([])
      expect(audit.done.value).toBe(true)
      expect(invoke.mock.calls.map(([command]) => command)).toEqual(['plugins_audit', 'plugins_audit_clear', 'plugins_audit_clear'])
    }
    finally {
      scope.stop()
    }
  })

  it('refreshes a reopened view after an outstanding clear without restoring cached history', async () => {
    const pending = deferred<null>()
    const invoke = vi.fn().mockResolvedValueOnce(entries(20)).mockImplementationOnce(() => pending.promise).mockResolvedValueOnce(entries(1, 'after-clear'))
    const { scope, active, audit } = setup(invoke)
    try {
      await audit.setExpanded(true)
      const clearing = audit.clear()
      active.value = false
      active.value = true
      await audit.setExpanded(true)
      expect(invoke).toHaveBeenCalledTimes(2)
      pending.resolve(null)
      expect(await clearing).toBe(false)
      await vi.waitFor(() => expect(audit.rows.value?.[0]?.capability).toBe('after-clear'))
      expect(invoke).toHaveBeenLastCalledWith('plugins_audit', { pluginId: '@acme/one', limit: 20, offset: 0 })
    }
    finally {
      scope.stop()
    }
  })

  it('drops queued work and late errors after the page is disposed', async () => {
    const pending = deferred<unknown>()
    const invoke = vi.fn(() => pending.promise)
    const { scope, audit } = setup(invoke)
    const opening = audit.setExpanded(true)
    await audit.setExpanded(false)
    await audit.setExpanded(true)
    scope.stop()
    pending.resolve({ code: 'unavailable', message: 'error.unavailable', retryable: true })
    await opening
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(audit.rows.value).toBeUndefined()
    expect(audit.error.value).toBeUndefined()
    expect(audit.expanded.value).toBe(false)
  })
})
