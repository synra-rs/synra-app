import type { PlatformCapability, SynraClient } from '@synra/client'
import { APP_PROFILE, createMockClient, synraError } from '@synra/testkit'
import { describe, expect, it, vi } from 'vitest'
import { createInjection } from '@/ipc/injection'
import { asSynraError } from '@/ipc/injection/errors'
import grantsFixture from '../fixtures/capability-contract.json'
import errorFixture from '../fixtures/error-contract.json'
import { bridge, context, FEATURES } from './injection-test-helpers'

describe('grant decisions produced by Core', () => {
  it.each(grantsFixture)('$grants → $query ($allowed)', ({ grants, query, validGrants, allowed }) => {
    const facade = createInjection(context({ granted: grants }))
    expect(facade.env.can(query)).toBe(allowed)
    if (!validGrants) {
      expect(() => createMockClient({ capabilities: grants })).toThrow(TypeError)
      return
    }
    const mock = createMockClient({ capabilities: grants })
    expect(mock.env.can(query)).toBe(allowed)
  })
})

describe('client method, platform and grant queries', () => {
  it('advertises the same features as the App profile and refuses unavailable methods', async () => {
    const facade = createInjection(context())
    const mock = createMockClient({ profile: APP_PROFILE })
    for (const feature of FEATURES)
      expect(mock.env.hostSupports(feature)).toBe(facade.env.hostSupports(feature))
    expect(new Set(mock.device.supported)).toEqual(new Set(facade.device.supported))
    for (const client of [facade, mock]) {
      await expect(client.device.pick()).rejects.toMatchObject({ code: 'unsupported' })
      await expect(client.event.emit('notes.update')).rejects.toMatchObject({ code: 'unsupported' })
    }
  })

  it.each([
    { granted: false, platform: false, code: 'capability_denied' },
    { granted: false, platform: true, code: 'capability_denied' },
    { granted: true, platform: false, code: 'unsupported' },
    { granted: true, platform: true, code: null }
  ])('clipboard read with grant=$granted and platform=$platform', async ({ granted, platform, code }) => {
    const capabilities = granted ? ['platform:*'] : []
    const platformCapabilities: PlatformCapability[] = platform ? ['clipboard_read'] : []
    const invoke = vi.fn(async () => {
      if (!platform)
        throw synraError('unsupported', 'error.unsupported')
      return null
    })
    const facade = createInjection(context({ granted: capabilities, platformCapabilities, bridge: bridge({ invoke }) }))
    const mock = createMockClient({ profile: APP_PROFILE, capabilities, platformCapabilities })
    for (const client of [facade, mock]) {
      expect(client.env.can('platform:read-clipboard')).toBe(granted)
      expect(client.env.platformSupports('clipboard_read')).toBe(platform)
      const result = client.clipboard.read()
      if (code === null)
        await expect(result).resolves.toBe('')
      else
        await expect(result).rejects.toMatchObject({ code })
    }
    expect(invoke).toHaveBeenCalledTimes(granted ? 1 : 0)
  })

  it.each([false, true])('refuses a transfer grouping id after checking the grant (granted=%s)', async (granted) => {
    const capabilities = granted ? ['file:transfer:send'] : []
    const invoke = vi.fn()
    const facade = createInjection(context({ granted: capabilities, bridge: bridge({ invoke }) }))
    const mock = createMockClient({ profile: APP_PROFILE, capabilities })
    mock.stub('transfer', 'send', 'fixture-transfer')
    const request = { to: 'peer', file: new Blob(['x']), fileName: 'x.txt', contextId: 'group' }
    const refusal = granted
      ? { code: 'unsupported', details: { feature: 'transfer.send contextId' } }
      : { code: 'capability_denied' }

    for (const client of [facade, mock])
      await expect(client.transfer.send(request)).rejects.toMatchObject(refusal)

    expect(invoke).not.toHaveBeenCalled()
    expect(mock.calls).toMatchObject([{ ns: 'transfer', method: 'send', args: [request], refused: refusal }])
  })
})

function proxyTimeout(get: () => never): unknown {
  return new Proxy({ code: 'timeout', message: 'error.timeout', retryable: true }, { get })
}

describe('error shapes through App and testkit', () => {
  async function rejectedByBoth(reason: unknown): Promise<unknown[]> {
    const mock = createMockClient({ profile: APP_PROFILE, capabilities: ['device:list'] })
    mock.stubFailure('device', 'list', reason)
    const invoke = vi.fn(async () => {
      throw reason
    })
    const facade = createInjection(context({ bridge: bridge({ invoke }) }))
    const errors: unknown[] = []
    for (const client of [facade, mock])
      await client.device.list().catch((error: unknown) => errors.push(error))
    expect(errors).toHaveLength(2)
    return errors
  }

  it('redacts credential-bearing diagnostic keys and keeps the first normalized key', async () => {
    const reason = synraError('internal', 'error.internal')
    const details = {
      'https://fixture-user:fixture-pass@registry.test/pkg?sig=fixture-first': 503,
      'https://registry.test/pkg?sig=fixture-second': 504,
      'phase': 'metadata'
    }
    for (const error of await rejectedByBoth({ ...reason, details })) {
      expect(error).toMatchObject({ code: 'internal', details: { 'https://registry.test/pkg': 503, 'phase': 'metadata' } })
      expect(JSON.stringify(error)).not.toContain('fixture-')
    }
  })

  it.each([
    `registry refused ${JSON.stringify({ token: 'fixture-quoted "value"', status: 503 })}`,
    'registry refused {\'api-key\': \'fixture-single value\', \'status\': 503}',
    `registry refused {"password":"${'fixture-cut '.repeat(90)}"}`
  ])('redacts quoted credentials in a bridge rejection', async (message) => {
    for (const error of await rejectedByBoth(new Error(message))) {
      expect(error).toMatchObject({ code: 'internal', details: { detail: expect.stringContaining('[REDACTED]') } })
      expect(JSON.stringify(error)).not.toContain('fixture-')
    }
  })

  it('checks every Core retry policy and normalizes it through each call path', async () => {
    for (const error of errorFixture.policies) {
      const mock = createMockClient({ profile: APP_PROFILE, capabilities: ['device:list'] })
      mock.stubFailure('device', 'list', error)
      const invoke = vi.fn(async () => {
        throw error
      })
      const facade = createInjection(context({ bridge: bridge({ invoke }) }))
      if (!facade.isSynraError(error))
        throw new Error('Core emitted an invalid client error')
      expect(synraError(error.code, error.message)).toEqual(error)
      for (const client of [facade, mock]) {
        expect(client.isSynraError(error)).toBe(true)
        await expect(client.device.list()).rejects.toEqual(error)
      }
    }
  })

  it('classifies invalid or unknown failures without running a property getter', async () => {
    const get = vi.fn(() => {
      throw new Error('direct property read')
    })
    const error = proxyTimeout(get)
    const unknowns = [null, undefined, 42, 'broken', new Error('transport failed'), { code: 'future', message: 'future', retryable: false }]
    const facade = createInjection(context())
    const mock = createMockClient({ profile: APP_PROFILE, capabilities: ['device:list'] })
    for (const client of [facade, mock] satisfies SynraClient[]) {
      expect(client.isCode(error, 'timeout')).toBe(true)
      expect(client.isCapabilityDenied(error)).toBe(false)
      for (const value of unknowns)
        expect(client.isSynraError(value)).toBe(false)
    }
    expect(get).not.toHaveBeenCalled()
    for (const value of unknowns) {
      mock.stubFailure('device', 'list', value)
      await expect(mock.device.list()).rejects.toEqual(asSynraError(value))
    }
  })
})
