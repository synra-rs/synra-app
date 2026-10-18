/**
 * What the plugins page asks the kernel, driven through a fake bridge.
 *
 * Three claims per call: the **command name**, the **argument shape**, and that a refusal comes back as something the
 * notice can render. The page itself is untested (no DOM harness), so these are the only assertions standing between the
 * buttons and the kernel's contract.
 */

import { describe, expect, it, vi } from 'vitest'

import { KernelClient } from '../../ipc/client'
import { decide, installFrom, planSync, runSync, setEnabled, uninstall } from './actions'

/** A client whose bridge answers with `answer` and records every call. */
function client(answer: unknown = { ok: true }) {
  const invoke = vi.fn(async (_command: string, _args?: unknown) => answer)
  return {
    invoke,
    client: new KernelClient({
      invoke,
      listen: async () => () => {}
    } as never)
  }
}

describe('the install call', () => {
  it('sends the tagged path source the command expects', async () => {
    const { client: kernel, invoke } = client({ pluginId: '@acme/hello', version: '1.0.0', enabled: false })

    const outcome = await installFrom(kernel, '/tmp/package')

    expect(invoke).toHaveBeenCalledWith('plugins_install', {
      source: { source: 'path', path: '/tmp/package' }
    })
    expect(outcome.ok).toBe(true)
  })

  it('answers a refusal as the notice needs it', async () => {
    const { client: kernel } = client({
      code: 'invalid_argument',
      message: 'error.invalidArgument',
      retryable: false,
      actions: [{ kind: 'retry' }]
    })

    const outcome = await installFrom(kernel, '/tmp/package')

    if (outcome.ok)
      throw new Error('a refusal must not be reported as a success')
    expect(outcome.failure.code).toBe('invalid_argument')
    expect(outcome.failure.actions).toBeDefined()
  })
})

describe('the consent call', () => {
  it('sends the ticked capabilities, copied rather than by reference', async () => {
    const { client: kernel, invoke } = client({ pluginId: '@acme/hello' })
    const ticked = ['log:*']

    await decide(kernel, '@acme/hello', ticked)

    const sent = (invoke.mock.calls.at(0)?.[1] ?? {}) as { granted: string[] }
    expect(sent.granted).toEqual(['log:*'])
    // ★ Not the same array: the argument is reactive state on the page, and a copy makes the request a value.
    expect(sent.granted).not.toBe(ticked)
  })
})

describe('the enable call', () => {
  it('picks the command from the boolean', async () => {
    const { client: kernel, invoke } = client(null)

    await setEnabled(kernel, '@acme/hello', true)
    await setEnabled(kernel, '@acme/hello', false)

    expect(invoke.mock.calls[0]?.[0]).toBe('plugins_enable')
    expect(invoke.mock.calls[1]?.[0]).toBe('plugins_disable')
    expect(invoke.mock.calls[0]?.[1]).toEqual({ pluginId: '@acme/hello' })
  })
})

describe('the uninstall call', () => {
  it('sends the identifier and the data decision the user made', async () => {
    const { client: kernel, invoke } = client(null)

    await uninstall(kernel, '@acme/hello', false)

    expect(invoke).toHaveBeenCalledWith('plugins_uninstall', {
      pluginId: '@acme/hello',
      removeData: false
    })
  })
})

describe('the sync calls', () => {
  it('asks for a plan with the plugin id', async () => {
    const { client: kernel, invoke } = client({ pluginId: '@acme/hello', targets: [] })

    const outcome = await planSync(kernel, '@acme/hello')

    expect(invoke).toHaveBeenCalledWith('plugins_sync_plan', { pluginId: '@acme/hello' })
    expect(outcome.ok).toBe(true)
  })

  it('runs the plan it was given, under the key the command reads', async () => {
    const { client: kernel, invoke } = client({ updated: 1, unchanged: 0 })
    const plan = {
      pluginId: '@acme/hello',
      targets: [{ deviceId: '11111111-1111-4111-8111-111111111111', displayName: 'Laptop', needsArtifact: true, online: true }]
    }

    const outcome = await runSync(kernel, plan)

    expect(invoke).toHaveBeenCalledWith('plugins_sync_run', { plan })
    expect(outcome.ok).toBe(true)
  })

  it('answers the plan’s refusal as the notice needs it', async () => {
    const { client: kernel } = client({
      code: 'unsupported',
      message: 'error.unsupported',
      retryable: false
    })

    const outcome = await planSync(kernel, '@acme/hello')

    if (outcome.ok)
      throw new Error('a refusal must not be reported as a success')
    expect(outcome.failure.code).toBe('unsupported')
  })
})
