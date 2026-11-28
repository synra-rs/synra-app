import { describe, expect, it } from 'vitest'
import { registryProblem } from '@/features/plugins/registry-problem'
import { KernelClient } from '@/ipc/client'
import { FakeKernel } from '@/ipc/fake-kernel'
import { previewRegistry } from '@/ipc/preview-registry'
import { registryFixture } from './registry-fixture'

describe('rust registry contracts in the browser preview', () => {
  it.each(registryFixture.settings)('matches the native decision for $value', ({ present, value, outcome }) => {
    const parsed = previewRegistry(present ? value : undefined)
    const expected = registryProblem(outcome.error?.details)
    if (expected?.kind === 'invalidConfiguration') {
      expect(parsed.ok).toBe(false)
      if (!parsed.ok) {
        expect(registryProblem(parsed.error.details)).toEqual({
          ...expected,
          reason: expect.any(String)
        })
      }
    }
    else {
      expect(parsed.ok).toBe(true)
      if (parsed.ok)
        expect(parsed.sources).toEqual(outcome.sources?.map(({ kind, url }) => ({ kind, url })) ?? [])
    }
  })

  it.each(['plugins_catalog_search', 'plugins_request_registry_search'])('%s refuses a corrupt source and responds to new sources', async (command) => {
    const client = new KernelClient(new FakeKernel())
    const args = command === 'plugins_catalog_search'
      ? {
          query: { query: 'reader' }
        }
      : { query: 'reader' }
    const missing = await client.call(command, args)
    expect(missing).toEqual({ ok: false, error: registryFixture.notConfigured })
    const invalid = await client.call('config_set', {
      patch: {
        set: {
          'ui.locale': 'zh-CN',
          'plugins.registry': ['https://valid.example', 123]
        }
      }
    })
    expect(invalid.ok).toBe(false)
    expect(await client.call('config_get')).toEqual({
      ok: true,
      value: { values: {}, overridden: [] }
    })
    await client.call('config_set', {
      patch: {
        set: { 'plugins.registry': 'static:https://bucket.example/plugins' }
      }
    })
    expect(await client.call(command, args)).toEqual({ ok: false, error: registryFixture.searchUnsupported })
    await client.call('config_set', {
      patch: {
        set: { 'plugins.registry': 'https://registry.example' }
      }
    })
    const answer = await client.call<unknown[]>(command, args)
    expect(answer.ok && answer.value.length).toBeGreaterThan(0)
  })
})
