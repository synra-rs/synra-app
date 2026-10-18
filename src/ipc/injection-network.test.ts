/**
 * The network namespace, over the plugin channel.
 *
 * # What this file is about
 *
 * The façade's half of `fetch`: the arguments it forwards, the two values it refuses locally, and the shape it turns the
 * kernel's answer into. The kernel's half — the host allow-list, the IP guard, redirects, the size ceiling — has its own
 * tests in `synra-fetch`; repeating them against a fake bridge would assert this file's copy of rules it does not own.
 */

import { describe, expect, it, vi } from 'vitest'

import { createInjection } from './injection'
import { bridge, context } from './injection-test-helpers'

/** A kernel answer shaped like the host's `Response` JSON. */
function answer(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    status: 200,
    statusText: 'OK',
    headers: { 'content-type': 'application/json' },
    url: 'https://api.example.com/v1',
    redirected: false,
    body: '{"ok":true}',
    ...overrides
  }
}

/** A bridge that answers `network.fetch` with this value and records the call. */
function answering(value: unknown) {
  return vi.fn<(command: string, payload?: Readonly<Record<string, unknown>>) => Promise<unknown>>(
    async (command, payload) => {
      expect(command).toBe('plugin_ui_rpc')
      expect(payload?.method).toBe('network.fetch')
      return value
    }
  )
}

describe('network.fetch', () => {
  it('carries the input and the initialiser over the channel', async () => {
    const invoke = answering(answer())
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await synra.network.fetch('https://api.example.com/v1', {
      method: 'POST',
      headers: { accept: 'application/json' },
      body: '{}'
    })

    expect(invoke).toHaveBeenCalledWith('plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'network.fetch',
      args: {
        input: 'https://api.example.com/v1',
        init: { method: 'POST', headers: { accept: 'application/json' }, body: '{}' }
      }
    })
  })

  it('answers a response with the contract fields', async () => {
    const synra = createInjection(context({ bridge: bridge({ invoke: answering(answer()) }) }))

    const response = await synra.network.fetch('https://api.example.com/v1')

    expect(response.status).toBe(200)
    expect(response.statusText).toBe('OK')
    expect(response.ok).toBe(true)
    expect(response.url).toBe('https://api.example.com/v1')
    expect(response.redirected).toBe(false)
    expect(response.headers).toEqual({ 'content-type': 'application/json' })
    await expect(response.json()).resolves.toEqual({ ok: true })
  })

  it('reads a non-2xx answer as a response rather than an error', async () => {
    // ★ `ok` is false and the call still resolves: a plugin asking about something that is not there has to read the
    // status and the body, and a rejection would make "no such thing" indistinguishable from "the network is down".
    const synra = createInjection(context({ bridge: bridge({ invoke: answering(answer({ status: 404, statusText: 'Not Found', body: 'missing' })) }) }))

    const response = await synra.network.fetch('https://api.example.com/missing')

    expect(response.status).toBe(404)
    expect(response.ok).toBe(false)
    await expect(response.text()).resolves.toBe('missing')
  })

  it('reports a redirect chain as the address it ended on', async () => {
    const synra = createInjection(context({ bridge: bridge({ invoke: answering(answer({ url: 'https://api.example.com/final', redirected: true })) }) }))

    const response = await synra.network.fetch('https://api.example.com/start')

    expect(response.url).toBe('https://api.example.com/final')
    expect(response.redirected).toBe(true)
  })

  it('reads bytes out of the text body', async () => {
    const synra = createInjection(context({ bridge: bridge({ invoke: answering(answer({ body: 'hi' })) }) }))

    const response = await synra.network.fetch('https://api.example.com/v1')

    await expect(response.bytes()).resolves.toEqual(new TextEncoder().encode('hi'))
  })

  it('rejects a second read and a body that is not JSON', async () => {
    const synra = createInjection(context({ bridge: bridge({ invoke: answering(answer({ body: 'not json' })) }) }))

    const response = await synra.network.fetch('https://api.example.com/v1')

    await expect(response.json()).rejects.toMatchObject({ code: 'invalid_argument' })
    await expect(response.text()).rejects.toMatchObject({ code: 'invalid_argument' })
  })

  it('refuses a byte body before the call', async () => {
    const invoke = answering(answer())
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await expect(synra.network.fetch('https://api.example.com', { body: new Uint8Array([1, 2]) }))
      .rejects
      .toMatchObject({ code: 'unsupported' })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('refuses a per-request timeout before the call', async () => {
    const invoke = answering(answer())
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await expect(synra.network.fetch('https://api.example.com', { timeoutMs: 500 }))
      .rejects
      .toMatchObject({ code: 'unsupported' })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('keeps a kernel refusal a refusal', async () => {
    const invoke = vi.fn<(command: string, payload?: Readonly<Record<string, unknown>>) => Promise<unknown>>(async () => {
      throw Object.assign(new Error('host not allowed'), {
        code: 'host_not_allowed',
        message: 'error.host_not_allowed'
      })
    })
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await expect(synra.network.fetch('https://blocked.example.com')).rejects.toMatchObject({
      code: 'host_not_allowed'
    })
  })
})
