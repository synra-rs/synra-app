/**
 * What the plugins page asks the kernel, driven through a fake bridge.
 *
 * Three claims per call: the **command name**, the **argument shape**, and that a refusal comes back as something the
 * notice can render. The page itself is untested (no DOM harness), so these are the only assertions standing between the
 * buttons and the kernel's contract.
 */

import { describe, expect, it, vi } from 'vitest'

import { KernelClient } from '../../ipc/client'
import { audit, clearAudit, decide, installFrom, planSync, runSync, setEnabled, uninstall } from './actions'

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

describe('the audit call', () => {
  it('asks for one plugin and a page', async () => {
    const { client: kernel, invoke } = client([{ atMs: 1, capability: 'log:*', allowed: true }])

    const outcome = await audit(kernel, '@acme/hello')

    expect(invoke).toHaveBeenCalledWith('plugins_audit', { pluginId: '@acme/hello', limit: 20, offset: 0 })
    expect(outcome.ok ? outcome.value.length : 0).toBe(1)
  })

  it('asks for the next page by offset, not by a different query', async () => {
    // ★ The page appends; a command that ignored the offset would make "Show more" redraw the rows already on screen.
    const { client: kernel, invoke } = client([{ atMs: 41, capability: 'log:*', allowed: true }])

    await audit(kernel, '@acme/hello', 20, 40)

    expect(invoke).toHaveBeenCalledWith('plugins_audit', { pluginId: '@acme/hello', limit: 20, offset: 40 })
  })

  it('answers a refusal as the notice needs it', async () => {
    // ★ `retryable` is part of the shape `isIpcError` checks, so a fixture without it is read as a **success** — which
    // is exactly how this test first failed, and the kind of failure a refusal fixture is meant to catch.
    const { client: kernel } = client({ code: 'capability_denied', message: 'error.capability_denied', retryable: false })

    const outcome = await audit(kernel, '@acme/hello')

    expect(outcome.ok).toBe(false)
    if (!outcome.ok)
      expect(outcome.failure.code).toBe('capability_denied')
  })

  it('keeps the newest-first order the command answers in', async () => {
    // ★ The command reads the table with `ORDER BY at_ms DESC, id DESC`, so the newest call is already first. This used to
    // be reversed in the page because the ring answered oldest first; keeping that flip would now show the oldest call of
    // the page at the top.
    const { client: kernel } = client([
      { atMs: 2, capability: 'network:fetch', allowed: false, reason: 'denied' },
      { atMs: 1, capability: 'log:*', allowed: true }
    ])

    const outcome = await audit(kernel, '@acme/hello')

    expect(outcome.ok ? outcome.value[0]?.capability : undefined).toBe('network:fetch')
  })
})

describe('the clear call', () => {
  it('forgets one plugin and sends nothing else', async () => {
    // ★ One identifier, because the ring is shared: a clear that carried a scope of "all" would be the button on one
    // card erasing another card's history.
    const { client: kernel, invoke } = client(null)

    const outcome = await clearAudit(kernel, '@acme/hello')

    expect(invoke).toHaveBeenCalledWith('plugins_audit_clear', { pluginId: '@acme/hello' })
    expect(outcome.ok).toBe(true)
  })

  it('answers a refusal as the notice needs it', async () => {
    const { client: kernel } = client({ code: 'unsupported', message: 'error.unsupported', retryable: false })

    const outcome = await clearAudit(kernel, '@acme/hello')

    expect(outcome.ok).toBe(false)
    if (!outcome.ok)
      expect(outcome.failure.code).toBe('unsupported')
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

  it('sends the destructive half of the decision too', async () => {
    // ★ The other answer: `removeData: true` is the one that deletes user files, and a page that could only send `false`
    // made the option unreachable — the command had always accepted it.
    const { client: kernel, invoke } = client(null)

    await uninstall(kernel, '@acme/hello', true)

    expect(invoke).toHaveBeenCalledWith('plugins_uninstall', {
      pluginId: '@acme/hello',
      removeData: true
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
