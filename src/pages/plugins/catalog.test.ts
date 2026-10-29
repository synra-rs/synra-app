import type { CatalogItem } from '@synra/protocol'
import { describe, expect, it, vi } from 'vitest'
import { effectScope } from 'vue'
import { KernelClient } from '../../ipc/client'
import { useCatalog } from './catalog'

function page(start: number, length = 20): CatalogItem[] {
  return Array.from({ length }, (_, i) => ({
    pluginId: `@acme/p${start + i}`,
    title: `Plugin ${start + i}`,
    version: '1.0.0',
    publisher: 'acme',
    capabilityCount: 0,
    installed: false
  }))
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function setup(invoke: (command: string, args?: unknown) => Promise<unknown>) {
  const scope = effectScope()
  const kernel = new KernelClient({ invoke, listen: async () => () => {} })
  const catalog = scope.run(() => useCatalog(kernel))!
  return { scope, catalog }
}

describe('registry search state', () => {
  it.each(['text', 'sort'] as const)('discards a late result after the %s draft changes and bounds outstanding reads', async (field) => {
    const pending = deferred<CatalogItem[]>()
    const invoke = vi.fn(() => pending.promise)
    const { scope, catalog } = setup(invoke)
    try {
      catalog.text.value = 'first'
      const first = catalog.search()
      if (field === 'text')
        catalog.text.value = 'next'
      else
        catalog.sort.value = 'name'
      await catalog.search()
      expect(invoke).toHaveBeenCalledTimes(1)
      expect(catalog.busy.value).toBe(true)
      pending.resolve(page(0))
      await first
      expect(catalog.results.value).toEqual([])
      expect(catalog.searched.value).toBe(false)
      expect(catalog.hasMore.value).toBe(false)
      expect(catalog.busy.value).toBe(false)
      await catalog.search()
      expect(invoke).toHaveBeenLastCalledWith('plugins_catalog_search', {
        query: { query: field === 'text' ? 'next' : 'first', sort: field === 'sort' ? 'name' : 'relevance', offset: 0 }
      })
    }
    finally {
      scope.stop()
    }
  })

  it('counts server rows across repeated results and retains a full continuation page', async () => {
    const invoke = vi.fn()
      .mockResolvedValueOnce(page(0))
      .mockResolvedValueOnce(page(19))
      .mockResolvedValueOnce([])
    const { scope, catalog } = setup(invoke)
    try {
      catalog.text.value = 'plugin'
      await catalog.search()
      await catalog.loadMore()
      expect(catalog.results.value).toHaveLength(39)
      expect(new Set(catalog.results.value.map(item => item.pluginId)).size).toBe(39)
      expect(catalog.hasMore.value).toBe(true)
      await catalog.loadMore()
      expect(invoke).toHaveBeenLastCalledWith('plugins_catalog_search', { query: { query: 'plugin', sort: 'relevance', offset: 40 } })
      expect(catalog.hasMore.value).toBe(false)
      expect(catalog.results.value).toHaveLength(39)
    }
    finally {
      scope.stop()
    }
  })

  it('keeps existing rows after a pagination refusal and retries the same offset', async () => {
    const invoke = vi.fn()
      .mockResolvedValueOnce(page(0))
      .mockRejectedValueOnce({ code: 'unavailable', message: 'error.unavailable', retryable: true })
      .mockResolvedValueOnce(page(20, 1))
    const { scope, catalog } = setup(invoke)
    try {
      catalog.text.value = 'plugin'
      await catalog.search()
      await catalog.loadMore()
      expect(catalog.results.value).toHaveLength(20)
      expect(catalog.error.value?.code).toBe('unavailable')
      expect(catalog.hasMore.value).toBe(true)
      await catalog.retry()
      expect(invoke).toHaveBeenLastCalledWith('plugins_catalog_search', { query: { query: 'plugin', sort: 'relevance', offset: 20 } })
      expect(catalog.results.value).toHaveLength(21)
      expect(catalog.error.value).toBeUndefined()
    }
    finally {
      scope.stop()
    }
  })

  it('retries a failed first page as a search even while previous rows remain', async () => {
    const invoke = vi.fn()
      .mockResolvedValueOnce(page(0))
      .mockRejectedValueOnce({ code: 'unavailable', message: 'error.unavailable', retryable: true })
      .mockResolvedValueOnce(page(40, 1))
    const { scope, catalog } = setup(invoke)
    try {
      catalog.text.value = 'plugin'
      await catalog.search()
      await catalog.search()
      expect(catalog.results.value).toHaveLength(20)
      await catalog.retry()
      expect(invoke).toHaveBeenLastCalledWith('plugins_catalog_search', { query: { query: 'plugin', sort: 'relevance', offset: 0 } })
      expect(catalog.results.value.map(item => item.pluginId)).toEqual(['@acme/p40'])
    }
    finally {
      scope.stop()
    }
  })

  it('discards a refusal for a draft that has already changed', async () => {
    const pending = deferred<unknown>()
    const invoke = vi.fn(() => pending.promise)
    const { scope, catalog } = setup(invoke)
    try {
      catalog.text.value = 'old'
      const request = catalog.search()
      catalog.text.value = 'new'
      // The bridge can also resolve with an IPC error value; KernelClient normalizes that refusal.
      pending.resolve({ code: 'unavailable', message: 'error.unavailable', retryable: true })
      await request
      expect(catalog.error.value).toBeUndefined()
      expect(catalog.searched.value).toBe(false)
      expect(catalog.busy.value).toBe(false)
    }
    finally {
      scope.stop()
    }
  })

  it('does not publish results after leaving the page', async () => {
    const pending = deferred<CatalogItem[]>()
    const { scope, catalog } = setup(() => pending.promise)
    catalog.text.value = 'plugin'
    const request = catalog.search()
    scope.stop()
    pending.resolve(page(0))
    await request
    expect(catalog.results.value).toEqual([])
    expect(catalog.searched.value).toBe(false)
    expect(catalog.error.value).toBeUndefined()
  })
})
