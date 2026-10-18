/**
 * The platform and clipboard namespaces, over the plugin channel.
 *
 * # What the grants are named
 *
 * `platform:open-url`, `platform:notify`, `platform:read-clipboard`, `platform:write-clipboard` — the capability
 * vocabulary the manifest declares and the kernel checks. The methods are `platform.openUrl`, `platform.notify`,
 * `clipboard.read` and `clipboard.write`; asserting both here is what keeps the two vocabularies apart.
 */

import { describe, expect, it, vi } from 'vitest'

import { createInjection } from './injection'
import { bridge, context } from './injection-test-helpers'

/** A bridge that answers this table by method and records the calls. */
function answering(answers: Record<string, (args: Readonly<Record<string, unknown>>) => unknown>) {
  return vi.fn<(command: string, payload?: Readonly<Record<string, unknown>>) => Promise<unknown>>(
    async (command, payload) => {
      expect(command).toBe('plugin_ui_rpc')
      const method = payload?.method as string
      const args = (payload?.args ?? {}) as Readonly<Record<string, unknown>>
      const answer = answers[method]
      if (answer === undefined) {
        throw Object.assign(new Error(`no answer for ${method}`), {
          code: 'unsupported',
          message: 'error.unsupported'
        })
      }
      return answer(args)
    }
  )
}

describe('platform', () => {
  it('opens a URL through the channel', async () => {
    const invoke = answering({ 'platform.openUrl': () => null })
    const synra = createInjection(context({ bridge: bridge({ invoke }), granted: ['platform:open-url'] }))

    await synra.platform.openUrl('https://example.com')

    expect(invoke).toHaveBeenCalledWith('plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'platform.openUrl',
      args: { url: 'https://example.com' }
    })
  })

  it('notifies through the channel', async () => {
    const invoke = answering({ 'platform.notify': () => null })
    const synra = createInjection(context({ bridge: bridge({ invoke }), granted: ['platform:notify'] }))

    await synra.platform.notify({ title: 'Build', body: 'it finished' })

    expect(invoke).toHaveBeenCalledWith('plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'platform.notify',
      args: { title: 'Build', body: 'it finished' }
    })
  })

  it('names the capability when the user did not grant it', async () => {
    // ★ The refusal is local and synchronous, so a page does not need a round trip to discover what it may not do. The
    // kernel checks the same capability again — this one is for the author's `catch`, not for the authority.
    const invoke = answering({})
    const synra = createInjection(context({ bridge: bridge({ invoke }), granted: [] }))

    await expect(synra.platform.openUrl('https://example.com')).rejects.toMatchObject({
      code: 'capability_denied',
      details: { capability: 'platform:open-url' }
    })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('keeps a kernel refusal a refusal', async () => {
    const invoke = vi.fn<(command: string, payload?: Readonly<Record<string, unknown>>) => Promise<unknown>>(async () => {
      throw Object.assign(new Error('the system refused'), {
        code: 'internal',
        message: 'error.internal'
      })
    })
    const synra = createInjection(context({ bridge: bridge({ invoke }), granted: ['platform:open-url'] }))

    await expect(synra.platform.openUrl('https://example.com')).rejects.toMatchObject({ code: 'internal' })
  })
})

describe('clipboard', () => {
  it('reads the text the kernel answered with', async () => {
    const invoke = answering({ 'clipboard.read': () => 'from the clipboard' })
    const synra = createInjection(context({ bridge: bridge({ invoke }), granted: ['platform:read-clipboard'] }))

    await expect(synra.clipboard.read()).resolves.toBe('from the clipboard')
  })

  it('turns an empty clipboard into an empty string', async () => {
    // The contract says `read(): Promise<string>`; `null` is "nothing is copied", which is an ordinary answer rather
    // than the error a `catch` would make it.
    const synra = createInjection(
      context({ bridge: bridge({ invoke: answering({ 'clipboard.read': () => null }) }), granted: ['platform:read-clipboard'] })
    )

    await expect(synra.clipboard.read()).resolves.toBe('')
  })

  it('writes the text through the channel', async () => {
    const invoke = answering({ 'clipboard.write': () => null })
    const synra = createInjection(context({ bridge: bridge({ invoke }), granted: ['platform:write-clipboard'] }))

    await synra.clipboard.write('hello')

    expect(invoke).toHaveBeenCalledWith('plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'clipboard.write',
      args: { text: 'hello' }
    })
  })

  it('refuses a read without the read capability', async () => {
    const invoke = answering({})
    const synra = createInjection(context({ bridge: bridge({ invoke }), granted: [] }))

    await expect(synra.clipboard.read()).rejects.toMatchObject({
      code: 'capability_denied',
      details: { capability: 'platform:read-clipboard' }
    })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('refuses a write without the write capability', async () => {
    const invoke = answering({})
    const synra = createInjection(context({ bridge: bridge({ invoke }), granted: ['platform:read-clipboard'] }))

    await expect(synra.clipboard.write('hello')).rejects.toMatchObject({
      code: 'capability_denied',
      details: { capability: 'platform:write-clipboard' }
    })
    expect(invoke).not.toHaveBeenCalled()
  })
})
