/**
 * The self namespace, over the plugin channel.
 *
 * # What is asserted
 *
 * The arguments each method sends, the shape `actions` accepts, and that a kernel refusal stays a refusal. What the
 * **kernel** checks — that the action belongs to this plugin, and that the headless half holds the grants the action
 * then uses — has its own tests in `src-tauri`; a fake bridge cannot re-check it and pretending to would test this file's
 * copy of a rule it does not own.
 */

import { describe, expect, it, vi } from 'vitest'

import { createInjection } from './injection'
import { bridge, context } from './injection-test-helpers'

/** A bridge that answers by method and records the call. */
function answering(answers: Readonly<Record<string, unknown>>) {
  return vi.fn<(command: string, payload?: Readonly<Record<string, unknown>>) => Promise<unknown>>(
    async (command, payload) => {
      expect(command).toBe('plugin_ui_rpc')
      const method = payload?.method as string
      if (!(method in answers)) {
        throw Object.assign(new Error(`no answer for ${method}`), {
          code: 'unsupported',
          message: 'error.unsupported'
        })
      }
      return answers[method]
    }
  )
}

describe('self', () => {
  it('lists the actions the kernel says this plugin declared', async () => {
    const invoke = answering({
      'self.actions': [
        { name: 'send', args: { type: 'object' }, description: 'Send a message' },
        { name: 'clear' }
      ]
    })
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await expect(synra.self.actions()).resolves.toEqual([
      { name: 'send', args: { type: 'object' }, description: 'Send a message' },
      { name: 'clear' }
    ])
    expect(invoke).toHaveBeenCalledWith('plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'self.actions',
      args: {}
    })
  })

  it('passes the action and its arguments', async () => {
    const invoke = answering({ 'self.invoke': { sent: true } })
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await expect(synra.self.invoke('send', { to: 'bob' })).resolves.toEqual({ sent: true })

    expect(invoke).toHaveBeenCalledWith('plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'self.invoke',
      args: { action: 'send', args: { to: 'bob' } }
    })
  })

  it('omits arguments an action does not take', async () => {
    // ★ Omitted rather than `undefined`: JSON drops an undefined value, and the kernel's missing field means `null` —
    // "this action takes nothing". Sending the key with `undefined` would make the wire shape depend on serialisation.
    const invoke = answering({ 'self.invoke': null })
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await synra.self.invoke('clear')

    expect(invoke).toHaveBeenCalledWith('plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'self.invoke',
      args: { action: 'clear' }
    })
  })

  it('refuses a list that is not a list', async () => {
    // The kernel's contract is an array. A page that received an object and called `.map` on it would reach a
    // `TypeError` with no code; this names the host as the thing that broke its promise.
    const synra = createInjection(context({ bridge: bridge({ invoke: answering({ 'self.actions': { not: 'a list' } }) }) }))

    await expect(synra.self.actions()).rejects.toMatchObject({ code: 'unsupported' })
  })

  it('keeps a kernel refusal a refusal', async () => {
    const invoke = vi.fn<(command: string, payload?: Readonly<Record<string, unknown>>) => Promise<unknown>>(async () => {
      throw Object.assign(new Error('that action is not this plugin\'s'), {
        code: 'invalid_argument',
        message: 'error.invalid_argument'
      })
    })
    const synra = createInjection(context({ bridge: bridge({ invoke }) }))

    await expect(synra.self.invoke('not-mine')).rejects.toMatchObject({ code: 'invalid_argument' })
  })
})
