import type { IpcError, SynraError } from '@synra/protocol'
import { readFileSync } from 'node:fs'
import { describe, expect, it, vi } from 'vitest'
import { KernelClient } from '@/ipc/client'
import { asSynraError, isSynraErrorValue } from '@/ipc/errors'
import { ERROR_RETRYABLE } from '@/ipc/errors/catalog'
import { createInjection } from '@/ipc/injection'
import { bridge, context } from './injection-test-helpers'

// Produced by synra-ipc/examples/error_contract.rs; shapes executes that producer and compares this snapshot.
const fixture = JSON.parse(readFileSync(new URL('../fixtures/error-contract.json', import.meta.url), 'utf8')) as {
  policies: IpcError[]
  ipc: IpcError
  plugin: SynraError
}

describe('real Rust error serialization', () => {
  it('covers every code and the retry policy used by local refusal constructors', () => {
    expect(Object.fromEntries(fixture.policies.map(error => [error.code, error.retryable]))).toEqual(ERROR_RETRYABLE)
    for (const error of fixture.policies) {
      expect(isSynraErrorValue(error)).toBe(true)
      expect(asSynraError(error)).toEqual(error)
    }
  })

  it('preserves real suggested actions through a namespace rejection and the host client', async () => {
    const seam = bridge({ invoke: vi.fn(async () => {
      throw fixture.ipc
    }) })
    const expected = {
      code: fixture.ipc.code,
      retryable: fixture.ipc.retryable,
      actions: fixture.ipc.actions,
      details: { params: fixture.ipc.params, context: fixture.ipc.details }
    }
    await expect(createInjection(context({ bridge: seam })).device.list()).rejects.toMatchObject(expected)
    await expect(new KernelClient(seam).call('devices_snapshot')).resolves.toEqual({ ok: false, error: fixture.ipc })
  })

  it('accepts protocol errors including trace IDs and diagnostic arrays', () => {
    expect(asSynraError(fixture.plugin)).toEqual(fixture.plugin)
  })
})
