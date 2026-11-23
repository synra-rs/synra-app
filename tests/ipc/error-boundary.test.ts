import type { SynraError } from '@synra/protocol'
import { describe, expect, it, vi } from 'vitest'
import { KernelClient } from '@/ipc/client'
import { createInjection } from '@/ipc/injection'
import { asSynraError, isSynraErrorValue } from '@/ipc/injection/errors'
import { bridge, context } from './injection-test-helpers'

describe('untrusted error boundary', () => {
  it.each([null, undefined, false, 42, 'failure', [], new Error('failure')])('safely rejects %j', (value) => {
    expect(isSynraErrorValue(value)).toBe(false)
    expect(asSynraError(value)).toMatchObject({ code: 'internal', retryable: true })
  })

  it.each([
    { code: 'timeout' },
    { code: 'timeout', message: 'error.timeout' },
    { code: 'future_code', message: 'error.future', retryable: true },
    { code: 'timeout', message: 'error.timeout', retryable: 'true' },
    { code: 'timeout', message: 'error.timeout', retryable: true, actions: ['retry'] }
  ])('refuses malformed structured errors %j', (value) => {
    expect(isSynraErrorValue(value)).toBe(false)
    expect(asSynraError(value).code).toBe('internal')
  })

  it('does not execute getters or exception formatting hooks', () => {
    const getter = vi.fn(() => {
      throw new Error('getter executed')
    })
    const value = Object.defineProperties({}, {
      code: { get: getter },
      message: { get: getter },
      toString: { value: getter }
    })
    expect(isSynraErrorValue(value)).toBe(false)
    expect(asSynraError(value).code).toBe('internal')
    expect(getter).not.toHaveBeenCalled()
  })

  it('reads validated proxy data without invoking its get trap', () => {
    const get = vi.fn(() => {
      throw new Error('read through proxy')
    })
    const error = new Proxy({ code: 'timeout', message: 'error.timeout', retryable: true }, { get })
    expect(isSynraErrorValue(error)).toBe(true)
    expect(asSynraError(error)).toEqual({ code: 'timeout', message: 'error.timeout', retryable: true })
    expect(get).not.toHaveBeenCalled()
  })

  it('normalizes a snapshot without reading validated properties again', () => {
    const reads = new Map<PropertyKey, number>()
    const values = { code: 'timeout', message: 'error.timeout', retryable: true }
    const error = new Proxy(values, {
      getOwnPropertyDescriptor(target, key) {
        const count = (reads.get(key) ?? 0) + 1
        reads.set(key, count)
        if (count > 1)
          throw new Error('property changed after validation')
        return Object.getOwnPropertyDescriptor(target, key)
      }
    })
    expect(asSynraError(error)).toEqual(values)
  })

  it('handles revoked proxies without a secondary exception', () => {
    const { proxy, revoke } = Proxy.revocable([], {})
    revoke()
    expect(isSynraErrorValue(proxy)).toBe(false)
    expect(asSynraError(proxy)).toMatchObject({ code: 'internal', retryable: true })
  })

  it('preserves IPC parameters and diagnostics without promoting their fields', () => {
    const result = asSynraError({
      code: 'capability_denied',
      message: 'error.capability_denied',
      retryable: false,
      params: { field: 'capability' },
      details: { code: 'internal', capability: 'device:list', token: 'private-token' },
      actions: [{ kind: 'requestCapability', capability: 'device:list' }],
      traceId: 'trace-1'
    })
    expect(result).toMatchObject({
      code: 'capability_denied',
      retryable: false,
      traceId: 'trace-1',
      actions: [{ kind: 'requestCapability', capability: 'device:list' }],
      details: { params: { field: 'capability' }, context: { code: 'internal', capability: 'device:list', token: '[REDACTED]' } }
    })
    expect(JSON.stringify(result)).not.toContain('private-token')
  })

  it('bounds cyclic diagnostics and removes credential-bearing URL parts', () => {
    const cyclic: Record<string, unknown> = { url: 'https://user:pass@example.test/path?token=private#private' }
    cyclic.self = cyclic
    cyclic.description = 'x'.repeat(10000)
    const result = asSynraError({ code: 'internal', message: 'error.internal', retryable: true, details: cyclic })
    const encoded = JSON.stringify(result)
    expect(encoded.length).toBeLessThan(5000)
    expect(encoded).toContain('https://example.test/path')
    expect(encoded).not.toContain('private')
    expect(encoded).not.toContain('user:pass')
  })

  it('classifies a real namespace rejection and the host client consistently', async () => {
    const seam = bridge({ invoke: vi.fn(async () => {
      throw new Error('transport failed')
    }) })
    await expect(createInjection(context({ bridge: seam })).device.list()).rejects.toMatchObject({
      code: 'internal',
      retryable: true,
      details: { detail: 'transport failed' }
    })
    const answer = await new KernelClient(seam).call('devices_snapshot')
    expect(answer).toMatchObject({ ok: false, error: { code: 'internal', retryable: true } })
  })

  it('keeps valid error classifications and a canonical denial key', async () => {
    const error: SynraError = { code: 'device_offline', message: 'offline', retryable: true }
    expect(asSynraError(error)).toEqual(error)
    await expect(createInjection(context()).clipboard.read()).rejects.toMatchObject({
      code: 'capability_denied',
      message: 'error.capability_denied'
    })
  })

  it('does not treat an unknown resolved error code as a successful command answer', async () => {
    const seam = bridge({ invoke: vi.fn(async () => ({ code: 'future_error', message: 'error.future', retryable: true })) })
    await expect(new KernelClient(seam).call('devices_snapshot')).resolves.toMatchObject({
      ok: false,
      error: { code: 'internal', retryable: true }
    })
  })
})
