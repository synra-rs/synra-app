/**
 * Transfers, over the plugin channel.
 *
 * # What this file is about
 *
 * The façade encodes uploads, reads status from the host and delivers snapshots to subscribers.
 */

import type { TransferId } from '@synra/protocol'
import { describe, expect, it, vi } from 'vitest'

import { createSynraError } from '@/ipc/errors'
import { createInjection } from '@/ipc/injection'
import { bridge, context } from './injection-test-helpers'

const ID = 'b7f7f0c2-4a7e-4f1a-9a9a-2e2b6c1d0a11'

/** A bridge whose `invoke` answers by method and whose `listen` hands back the callback for driving events. */
function channel(answers: Readonly<Record<string, unknown>>) {
  const invoke = vi.fn<(command: string, payload?: Readonly<Record<string, unknown>>) => Promise<unknown>>(
    async (command, payload) => {
      expect(command).toBe('plugin_ui_rpc')
      const method = payload?.method as string
      if (!(method in answers))
        throw Object.assign(new Error(`no answer for ${method}`), { code: 'unsupported', message: 'error.unsupported' })
      const answer = answers[method]
      return typeof answer === 'function' ? answer(payload?.args) : answer
    }
  )
  return { bridge: bridge({ invoke }), invoke }
}

describe('transfer', () => {
  it('sends the blob as chunks and reads every status from the host', async () => {
    const answers = {
      'transfer.begin': { uploadId: 'UPLOAD1', chunkBytes: 3 },
      'transfer.chunk': ({ offset, data }: { offset: number, data: string }) => ({ written: offset + atob(data).length, totalBytes: 5 }),
      'transfer.finish': { transferId: ID },
      'transfer.status': { transferId: ID, state: 'transferring', receivedBytes: 2, totalBytes: 5, blocksDone: 1, blocksTotal: 2, revision: 2, endedAtMs: undefined as number | undefined }
    }
    const { bridge: seam, invoke } = channel(answers)
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
      args: { uploadId: 'UPLOAD1', offset: 0, data: 'aGVs' }
    })
    expect(invoke).toHaveBeenNthCalledWith(3, 'plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'transfer.chunk',
      args: { uploadId: 'UPLOAD1', offset: 3, data: 'bG8=' }
    })
    expect(invoke).toHaveBeenNthCalledWith(4, 'plugin_ui_rpc', {
      token: '123456789012345678901234567890',
      method: 'transfer.finish',
      args: { uploadId: 'UPLOAD1' }
    })

    await expect(synra.transfer.status(id)).resolves.toMatchObject({
      transferId: ID,
      state: 'transferring',
      receivedBytes: 2,
      totalBytes: 5
    })
    answers['transfer.status'] = { transferId: ID, state: 'completed', receivedBytes: 5, totalBytes: 5, blocksDone: 2, blocksTotal: 2, revision: 4, endedAtMs: 100 }
    await expect(synra.transfer.status(id)).resolves.toMatchObject({ state: 'completed', receivedBytes: 5 })
    expect(invoke).toHaveBeenNthCalledWith(6, 'plugin_ui_rpc', expect.objectContaining({
      method: 'transfer.status',
      args: { transferId: ID }
    }))
  })

  it('refuses a send without the grant, before reading the file', async () => {
    const { bridge: seam, invoke } = channel({})
    const synra = createInjection(context({ bridge: seam, granted: [] }))

    await expect(
      synra.transfer.send({ to: 'peer', file: new Blob(['x']), fileName: 'a.txt' })
    ).rejects.toMatchObject({ code: 'capability_denied' })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('propagates the host size refusal before reading the blob', async () => {
    const { bridge: seam, invoke } = channel({
      'transfer.begin': () => { throw createSynraError('payload_too_large', { maxBytes: 40 }) }
    })
    const synra = createInjection(context({ bridge: seam, granted: ['file:transfer:send'] }))
    const slice = vi.fn()
    const huge = { size: 41, slice } as unknown as Blob

    await expect(
      synra.transfer.send({ to: 'peer', file: huge, fileName: 'big.bin' })
    ).rejects.toMatchObject({ code: 'payload_too_large' })
    expect(invoke).toHaveBeenCalledExactlyOnceWith('plugin_ui_rpc', expect.objectContaining({ method: 'transfer.begin' }))
    expect(slice).not.toHaveBeenCalled()
  })

  it('refuses the grouping id this channel does not carry', async () => {
    const { bridge: seam } = channel({})
    const synra = createInjection(context({ bridge: seam, granted: ['file:transfer:send'] }))

    await expect(
      synra.transfer.send({ to: 'peer', file: new Blob(['x']), fileName: 'a.txt', contextId: 'c1' })
    ).rejects.toMatchObject({ code: 'unsupported' })
  })

  it('aborts an upload when the host acknowledgement disagrees with the written range', async () => {
    const { bridge: seam, invoke } = channel({
      'transfer.begin': { uploadId: 'UPLOAD1', chunkBytes: 3 },
      'transfer.chunk': { written: 1, totalBytes: 3 },
      'transfer.abort': { aborted: true }
    })
    const synra = createInjection(context({ bridge: seam, granted: ['file:transfer:send'] }))
    const sending = synra.transfer.send({ to: 'peer', file: new Blob(['abc']), fileName: 'a.txt' })
    await expect(sending).rejects.toMatchObject({ code: 'internal', details: { reason: 'invalid_upload_acknowledgement' } })
    expect(invoke.mock.calls.map(([, payload]) => payload?.method)).toEqual(['transfer.begin', 'transfer.chunk', 'transfer.abort'])
  })

  it('uses the acknowledged bytes even when the last block is short', async () => {
    const { bridge: seam } = channel({
      'transfer.status': {
        transferId: ID,
        state: 'transferring',
        receivedBytes: 68,
        totalBytes: 100,
        blocksDone: 3,
        blocksTotal: 4,
        revision: 4
      }
    })
    const synra = createInjection(context({ bridge: seam, granted: ['file:transfer:send'] }))

    await expect(synra.transfer.status(ID as TransferId)).resolves.toMatchObject({
      state: 'transferring',
      receivedBytes: 68,
      totalBytes: 100
    })
  })

  it('reads the host terminal state for a listener attached after the send', async () => {
    let finishPoll: (frame: unknown) => void = () => {}
    const { bridge: seam } = channel({
      'transfer.begin': { uploadId: 'UPLOAD1', chunkBytes: 10 },
      'transfer.chunk': { written: 2, totalBytes: 2 },
      'transfer.finish': { transferId: ID },
      'event.subscribe': 'stream',
      'event.unsubscribe': null,
      'event.poll': () => new Promise(resolve => finishPoll = resolve),
      'transfer.status': { transferId: ID, state: 'completed', receivedBytes: 2, totalBytes: 2, blocksDone: 1, blocksTotal: 1, revision: 3, endedAtMs: 100 }
    })
    const synra = createInjection(context({ bridge: seam, granted: ['file:transfer:send'] }))

    await synra.transfer.send({ to: 'peer', file: new Blob(['hi']), fileName: 'a.txt' })
    const seen: unknown[] = []
    const off = synra.transfer.onProgress(ID as TransferId, progress => seen.push(progress))
    await vi.waitFor(() => {
      expect(seen).toEqual([
        { transferId: ID, state: 'completed', receivedBytes: 2, totalBytes: 2, blocksDone: 1, blocksTotal: 1, revision: 3, endedAtMs: 100, ratio: 1 }
      ])
    })
    off()
    finishPoll(null)
  })
})
