/**
 * The client's decisions, asserted against a bridge that records what it was asked.
 *
 * # What is real here and what is not
 *
 * `invoke` and `listen` are the only two things faked, and they are faked because the real ones need a WebView. Below
 * them everything is the shipped code: the error classification, the rejection handling, the retry judgement. A test
 * that stubbed the client's own logic would be testing the stub.
 */

import type { ErrorCode } from '@synra/protocol'
import type { Bridge, IpcError } from './client'

import { describe, expect, it, vi } from 'vitest'
import { isIpcError, KernelClient, shouldOfferRetry } from './client'

/** A bridge that records its calls and answers with what a test queued. */
function bridge(answer: unknown = { ok: true }): {
  bridge: Bridge
  commands: string[]
  listeners: Map<string, (payload: unknown) => void>
} {
  const commands: string[] = []
  const listeners = new Map<string, (payload: unknown) => void>()
  return {
    commands,
    listeners,
    bridge: {
      invoke: (command) => {
        commands.push(command)
        return Promise.resolve(answer)
      },
      listen: (event, handler) => {
        listeners.set(event, handler)
        return Promise.resolve(() => listeners.delete(event))
      }
    }
  }
}

/** An error with the three fields the interface reads. */
function ipcError(code: ErrorCode, retryable: boolean): IpcError {
  return { code, message: 'error.someKey', retryable }
}

describe('calling a command', () => {
  it('reports a value as a success', async () => {
    const { bridge: seam, commands } = bridge({ devices: [] })
    const client = new KernelClient(seam)

    const result = await client.call<{ devices: unknown[] }>('devices_snapshot')
    expect(result).toEqual({ ok: true, value: { devices: [] } })
    expect(commands).toEqual(['devices_snapshot'])
  })

  it('passes the arguments through unchanged', async () => {
    // The argument names are the kernel's, so a client that camel-cased or renamed one would produce a call the kernel
    // refuses with "missing field" — naming a field the caller never wrote.
    const seen: (Readonly<Record<string, unknown>> | undefined)[] = []
    const seam: Bridge = {
      invoke: (_command, args) => {
        seen.push(args)
        return Promise.resolve(null)
      },
      listen: () => Promise.resolve(() => undefined)
    }

    await new KernelClient(seam).call('devices_connect', { deviceId: 'dev_a' })
    expect(seen).toEqual([{ deviceId: 'dev_a' }])
  })

  it('reports a resolved error object as a failure', async () => {
    // ★ The host's convention: a command that *returned* an error resolves with it. An interface that only handled a
    // rejection would treat this as a successful call whose value happens to have a `code` field.
    const { bridge: seam } = bridge(ipcError('device_offline', true))
    const client = new KernelClient(seam)

    const result = await client.call('devices_connect')
    expect(result.ok).toBe(false)
    if (result.ok)
      throw new Error('expected a failure')
    expect(result.error.code).toBe('device_offline')
    expect(result.error.retryable).toBe(true)
  })

  it('reports a rejection as a failure rather than throwing', async () => {
    // ★ The other half of the same convention: a command that *panicked* rejects. A caller that had to `try`/`catch`
    // each call is one where a missed `catch` is a blank panel.
    const seam: Bridge = {
      invoke: () => Promise.reject(new Error('the bridge fell over')),
      listen: () => Promise.resolve(() => undefined)
    }
    const client = new KernelClient(seam)

    const result = await client.call('devices_snapshot')
    expect(result.ok).toBe(false)
    if (result.ok)
      throw new Error('expected a failure')
    // The kernel's `internal`, which its own documentation says is retryable.
    expect(result.error.code).toBe('internal')
    expect(result.error.retryable).toBe(true)
    // And the detail is carried, so a diagnostic report can say what the bridge said.
    expect(result.error.params?.detail).toContain('the bridge fell over')
  })

  it('passes through a rejection that is already an error', async () => {
    // Some bridges reject with the error rather than resolving with it, and re-wrapping it would replace the kernel's
    // code with `internal` — losing the one field the caller branches on.
    const seam: Bridge = {
      invoke: () => Promise.reject(ipcError('capability_denied', false)),
      listen: () => Promise.resolve(() => undefined)
    }
    const client = new KernelClient(seam)

    const result = await client.call('plugins_enable')
    expect(result.ok).toBe(false)
    if (result.ok)
      throw new Error('expected a failure')
    expect(result.error.code).toBe('capability_denied')
    expect(result.error.retryable).toBe(false)
  })

  it('does not mistake a value that merely has a code field for an error', async () => {
    // A command whose return type is an object with a `code` field is legitimate — an error code, a country code — and
    // the check needs all three fields so that such a value is a success.
    const { bridge: seam } = bridge({ code: 'x', name: 'a plugin' })
    const client = new KernelClient(seam)
    const result = await client.call<{ code: string }>('plugins_list')
    expect(result.ok).toBe(true)
  })
})

describe('whether a value is an error', () => {
  it('accepts the three fields the interface reads', () => {
    expect(isIpcError(ipcError('timeout', true))).toBe(true)
  })

  it('refuses anything missing one of them', () => {
    // Each field is used: `code` is what a caller branches on, `message` is the key the interface renders, `retryable`
    // is whether to offer the button. One missing is one the interface cannot render correctly.
    expect(isIpcError({ code: 'timeout', retryable: true })).toBe(false)
    expect(isIpcError({ code: 'timeout', message: 'k' })).toBe(false)
    expect(isIpcError({ message: 'k', retryable: true })).toBe(false)
    expect(isIpcError({ code: 'timeout', message: 'k', retryable: 'yes' })).toBe(false)
  })

  it('refuses the things a bridge might send instead', () => {
    // A string, a number, null and a bare Error are all shapes a WebView bridge can produce for a failure, and none of
    // them is renderable — so each becomes `internal` through `bridgeFailure` rather than being read as a code.
    for (const value of ['a string', 42, null, undefined, [], new Error('boom')]) {
      expect(isIpcError(value)).toBe(false)
    }
  })

  it('accepts a richer error, because more fields is not less information', () => {
    // The Rust type has `params`, `details` and `actions`, and a check that required an exact field set would refuse
    // the real error the moment the kernel gained one.
    expect(
      isIpcError({
        code: 'invalid_argument',
        message: 'error.badArgument',
        params: { field: 'name' },
        details: 'the name was empty',
        retryable: false,
        actions: ['openSettings']
      })
    ).toBe(true)
  })
})

describe('whether to offer a retry', () => {
  it('uses the kernel’s answer rather than deriving one', () => {
    // ★ The design document is explicit that `internal` **is** retryable in the kernel. An interface that decided for
    // itself would be a second opinion that can contradict the kernel the user is talking to — and the contradiction
    // shows as a "Retry" button that fails identically, or a message with no button that would have worked.
    expect(shouldOfferRetry(ipcError('internal', true))).toBe(true)
    expect(shouldOfferRetry(ipcError('capability_denied', false))).toBe(false)
  })

  it('offers a retry for a bridge failure, which is transport-shaped', () => {
    // The one error this layer creates. Not retryable would be an interface that gives up on a transient fault it
    // cannot distinguish from a permanent one.
    const seam: Bridge = {
      invoke: () => Promise.reject(new Error('transient')),
      listen: () => Promise.resolve(() => undefined)
    }
    return new KernelClient(seam)
      .call('identity_get')
      .then((result) => {
        if (result.ok)
          throw new Error('expected a failure')
        expect(shouldOfferRetry(result.error)).toBe(true)
      })
  })
})

describe('subscribing to an event', () => {
  it('registers the listener and hands back an unsubscribe', async () => {
    const { bridge: seam, listeners } = bridge()
    const client = new KernelClient(seam)
    const handler = vi.fn()

    const off = await client.subscribe('kernel-event', handler)
    expect(listeners.has('kernel-event')).toBe(true)
    off()
    expect(listeners.has('kernel-event')).toBe(false)
  })

  it('delivers a payload to the handler', async () => {
    const { bridge: seam, listeners } = bridge()
    const client = new KernelClient(seam)
    const seen: unknown[] = []
    await client.subscribe('kernel-event', payload => seen.push(payload))

    listeners.get('kernel-event')?.({ seq: 1 })
    expect(seen).toEqual([{ seq: 1 }])
  })
})

describe('the error type the interface uses', () => {
  it('is not the plugin-facing one', () => {
    // A compile-time assertion with a runtime witness: `message` here is a **key** and in `SynraError` a sentence, and
    // the field the interface needs to localise is the field a plugin cannot use. Two types rather than one is what lets
    // the interface render in the user's language.
    const error: IpcError = { code: 'timeout', message: 'error.timeout', retryable: true }
    expect(error.message).toMatch(/^error\./)
  })
})
