/**
 * Calling a command and subscribing to an event, over a bridge that records what it was asked.
 *
 * # What is real here and what is not
 *
 * `invoke` and `listen` are the only two things faked, and they are faked because the real ones need a WebView. Below
 * them everything is the shipped code: the error classification, the rejection handling, the retry judgement. A test
 * that stubbed the client's own logic would be testing the stub.
 *
 * This file is the first of two: the calls and the subscription are here, and the classification of what came back —
 * `client-error.test.ts` — is beside it. The factories both use are in `client-test-helpers.ts`, which the runner does
 * not collect.
 */

import type { Bridge } from './client'

import { describe, expect, it, vi } from 'vitest'
import { KernelClient, shouldOfferRetry } from './client'
import { bridge, ipcError } from './client-test-helpers'

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
