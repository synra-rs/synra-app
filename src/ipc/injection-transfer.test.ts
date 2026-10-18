/**
 * Transfers, over the plugin channel.
 *
 * # What this file is about
 *
 * The façade's half: how the file becomes base64, what it does with the kernel's final state (which the registry no
 * longer holds), and the two ways progress reaches a listener — live events while sending, and a replay for one that
 * attached after `await send`. The wire behaviour of the send itself belongs to the desktop's transfer tests.
 */

import type { TransferId } from '@synra/protocol'
import { describe, expect, it, vi } from 'vitest'

import { createInjection } from './injection'
import { bridge, context } from './injection-test-helpers'

const ID = 'b7f7f0c2-4a7e-4f1a-9a9a-2e2b6c1d0a11'

/** A bridge whose `invoke` answers by method and whose `listen` hands back the callback for driving events. */
function channel(answers: Readonly<Record<string, unknown>>, onListen?: (callback: (payload: unknown) => void) => void) {
  const invoke = vi.fn<(command: string, payload?: Readonly<Record<string, unknown>>) => Promise<unknown>>(
    async (command, payload) => {
      expect(command).toBe('plugin_ui_rpc')
      const method = payload?.method as string
      if (!(method in answers))
        throw Object.assign(new Error(`no answer for ${method}`), { code: 'unsupported', message: 'error.unsupported' })
      return answers[method]
    }
  )
  const listen = vi.fn(async (_channel: string, callback: (payload: unknown) => void) => {
    onListen?.(callback)
    return () => {}
  })
  return { bridge: bridge({ invoke, listen }), invoke }
}

describe('transfer', () => {
  it('sends the blob as chunks and remembers how it ended', async () => {
    const { bridge: seam, invoke } = channel({
      'transfer.begin': { uploadId: 'UPLOAD1' },
      'transfer.chunk': {},
      'transfer.finish': { transferId: ID }
    })
    const synra = createInjection(context({ bridge: seam, granted: ['file:transfer:send'] }))

    const id = await synra.transfer.send({
      to: '11111111-1111-4111-8111-111111111111',
      file: new Blob(['hello']),
      fileName: 'note.txt'
    })

    expect(id).toBe(ID)
    expect(invoke).toHaveBeenNthCalledWith(1, 'plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'transfer.begin',
      args: {
        to: '11111111-1111-4111-8111-111111111111',
        fileName: 'note.txt',
        totalBytes: 5
      }
    })
    expect(invoke).toHaveBeenNthCalledWith(2, 'plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'transfer.chunk',
      args: { uploadId: 'UPLOAD1', data: 'aGVsbG8=' }
    })
    expect(invoke).toHaveBeenNthCalledWith(3, 'plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'transfer.finish',
      args: { uploadId: 'UPLOAD1' }
    })

    // ★ The call answers when the bytes are handed to the sender, so the state here is "on its way", not "arrived".
    await expect(synra.transfer.status(id)).resolves.toMatchObject({
      transferId: ID,
      state: 'transferring',
      receivedBytes: 0,
      totalBytes: 5
    })
  })

  it('refuses a send without the grant, before reading the file', async () => {
    const { bridge: seam, invoke } = channel({})
    const synra = createInjection(context({ bridge: seam, granted: [] }))

    await expect(
      synra.transfer.send({ to: 'peer', file: new Blob(['x']), fileName: 'a.txt' })
    ).rejects.toMatchObject({ code: 'capability_denied' })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('refuses a file larger than the chunked channel carries', async () => {
    const { bridge: seam, invoke } = channel({})
    const synra = createInjection(context({ bridge: seam, granted: ['file:transfer:send'] }))
    // A shaped object rather than 64 real megabytes: `send` decides on `size` before it reads anything.
    const huge = { size: 64 * 1024 * 1024 + 1, arrayBuffer: async () => new ArrayBuffer(0) } as unknown as Blob

    await expect(
      synra.transfer.send({ to: 'peer', file: huge, fileName: 'big.bin' })
    ).rejects.toMatchObject({ code: 'payload_too_large' })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('refuses the grouping id this channel does not carry', async () => {
    const { bridge: seam } = channel({})
    const synra = createInjection(context({ bridge: seam, granted: ['file:transfer:send'] }))

    await expect(
      synra.transfer.send({ to: 'peer', file: new Blob(['x']), fileName: 'a.txt', contextId: 'c1' })
    ).rejects.toMatchObject({ code: 'unsupported' })
  })

  it('derives bytes from the blocks when the sender only counted blocks', async () => {
    const { bridge: seam } = channel({
      'transfer.status': {
        transferId: ID,
        state: 'transferring',
        receivedBytes: 0,
        totalBytes: 100,
        blocksDone: 3,
        blocksTotal: 4
      }
    })
    const synra = createInjection(context({ bridge: seam, granted: ['file:transfer:send'] }))

    await expect(synra.transfer.status(ID as TransferId)).resolves.toMatchObject({
      state: 'transferring',
      receivedBytes: 75,
      totalBytes: 100
    })
  })

  it('delivers live progress for the transfer it was asked about', async () => {
    let listener: ((payload: unknown) => void) | undefined
    const { bridge: seam } = channel({}, (callback) => {
      listener = callback
    })
    const synra = createInjection(context({ bridge: seam, granted: ['file:transfer:send'] }))
    const seen: unknown[] = []

    const off = synra.transfer.onProgress(ID as TransferId, progress => seen.push(progress))

    listener?.({
      type: 'transferUpdated',
      payload: { transferId: ID, state: 'transferring', receivedBytes: 0, totalBytes: 10, blocksDone: 5, blocksTotal: 10 }
    })
    listener?.({
      type: 'transferUpdated',
      payload: { transferId: 'another', state: 'transferring', receivedBytes: 0, totalBytes: 10 }
    })

    expect(seen).toEqual([
      { transferId: ID, state: 'transferring', receivedBytes: 5, totalBytes: 10, ratio: 0.5 }
    ])

    off()
    listener?.({
      type: 'transferUpdated',
      payload: { transferId: ID, state: 'completed', receivedBytes: 10, totalBytes: 10 }
    })
    expect(seen).toHaveLength(1)
  })

  it('replays what it knows to a listener that attached after the send', async () => {
    const { bridge: seam } = channel({
      'transfer.begin': { uploadId: 'UPLOAD1' },
      'transfer.chunk': {},
      'transfer.finish': { transferId: ID }
    })
    const synra = createInjection(context({ bridge: seam, granted: ['file:transfer:send'] }))

    await synra.transfer.send({ to: 'peer', file: new Blob(['hi']), fileName: 'a.txt' })
    const seen: unknown[] = []
    synra.transfer.onProgress(ID as TransferId, progress => seen.push(progress))
    await Promise.resolve()

    expect(seen).toEqual([
      { transferId: ID, state: 'transferring', receivedBytes: 0, totalBytes: 2, ratio: 0 }
    ])
  })
})
