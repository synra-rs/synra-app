import type { IpcError } from '@synra/protocol'
import { describe, expect, it, vi } from 'vitest'
import { effectScope, ref } from 'vue'
import { useCatalog } from '@/features/plugins/catalog'
import { KernelClient } from '@/ipc/client'
import { registryFixture } from '../../ipc/registry-fixture'

describe('registry workflow through the real IPC client', () => {
  it.each(['error.unsupported', '未配置源', 'an unrelated future translation key'])('uses typed reasons independently of %s', async (message) => {
    const scope = effectScope()
    const error = { ...registryFixture.notConfigured, message }
    const client = new KernelClient({
      invoke: async () => {
        throw error
      },
      listen: async () => () => {}
    })
    const catalog = scope.run(() => useCatalog(client))!
    try {
      catalog.text.value = 'reader'
      await catalog.search()
      expect(catalog.state.value).toBe('notConfigured')
    }
    finally { scope.stop() }
  })

  it.each([
    ['invalidConfiguration', registryFixture.invalidConfiguration],
    ['searchUnsupported', registryFixture.searchUnsupported],
    ['failed', registryFixture.network],
    [
      'failed',
{ code: 'unsupported', message: 'no registry is configured', retryable: false } satisfies IpcError
    ]
  ] as const)('distinguishes %s from a missing configuration', async (state, error) => {
    const scope = effectScope()
    const client = new KernelClient({
      invoke: async () => {
        throw error
      },
      listen: async () => () => {}
    })
    const catalog = scope.run(() => useCatalog(client))!
    try {
      catalog.text.value = 'reader'
      await catalog.search()
      expect(catalog.state.value).toBe(state)
      expect(catalog.error.value).toMatchObject({ code: error.code, retryable: error.retryable })
      if (state === 'invalidConfiguration')
        expect(catalog.error.value?.detail).toContain('plugins.registry[1]')
    }
    finally { scope.stop() }
  })

  it('discards a prior source response and queries the current source on the next submission', async () => {
    let finish!: (value: unknown) => void
    const source = ref('https://old.example')
    const observed: string[] = []
    const invoke = vi.fn(() => {
      observed.push(source.value)
      return observed.length === 1
        ? new Promise(resolve => finish = resolve)
        : Promise.resolve([])
    })
    const scope = effectScope()
    const client = new KernelClient({
      invoke,
      listen: async () => () => {}
    })
    const catalog = scope.run(() => useCatalog(client, () => source.value))!
    try {
      catalog.text.value = 'reader'
      const pending = catalog.search()
      source.value = 'https://next.example'
      finish(registryFixture.notConfigured)
      await pending
      expect(catalog.state.value).toBe('idle')
      expect(catalog.error.value).toBeUndefined()
      await catalog.search()
      expect(observed).toEqual(['https://old.example', 'https://next.example'])
      expect(catalog.state.value).toBe('ready')
      expect(catalog.results.value).toEqual([])
    }
    finally { scope.stop() }
  })
})
