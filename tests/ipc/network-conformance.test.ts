import type { SynraErrorCode, SynraFetchMethod } from '@synra/client'
import { APP_PROFILE, createMockClient, createMockResponse, synraError } from '@synra/testkit'
import { describe, expect, it, vi } from 'vitest'
import { createInjection } from '@/ipc/injection'
import fixture from '../fixtures/network-scope.json'
import { bridge, context } from './injection-test-helpers'

describe('network scope shared by the host and testkit', () => {
  it.each(fixture)('$name', async ({ input, method, grants, code, appCode, capability }) => {
    const init = method === undefined
      ? undefined
      : { method: method as SynraFetchMethod }
    const mock = createMockClient({ profile: APP_PROFILE, capabilities: grants })
    mock.stub('network', 'fetch', createMockResponse('allowed'))
    const result = mock.network.fetch(input, init)
    if (code === undefined) {
      await expect(result.then(response => response.text())).resolves.toBe('allowed')
    }
    else if (capability === undefined) {
      await expect(result).rejects.toMatchObject({ code })
    }
    else {
      await expect(result).rejects.toMatchObject({
        code,
        actions: [
          { kind: 'requestCapability', capability }
        ]
      })
    }
    expect(mock.calls).toHaveLength(1)

    // The Rust RPC suite asserts the same wire code with its real host and persisted grants.
    const wireCode = appCode ?? code
    if (wireCode === undefined)
      throw new Error('missing host outcome')
    const invoke = vi.fn(async () => {
      throw synraError(wireCode as SynraErrorCode, `error.${wireCode}`)
    })
    const facade = createInjection(context({
      granted: grants,
      bridge: bridge({ invoke })
    }))
    await expect(facade.network.fetch(input, init)).rejects.toMatchObject({ code: wireCode })
    expect(invoke).toHaveBeenCalledExactlyOnceWith('plugin_ui_rpc', expect.objectContaining({
      method: 'network.fetch',
      args: {
        input,
        init: { method, headers: undefined, body: undefined }
      }
    }))
  })
})
