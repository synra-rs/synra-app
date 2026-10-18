/**
 * Transfers, over the plugin channel.
 *
 * # Why the page encodes the file itself
 *
 * `transfer.send` takes a `Blob`, and an IPC call is JSON. The first slice therefore sends the bytes as base64 and the
 * kernel decodes them into a temporary file before handing them to the same dial, block loop and progress events the
 * transfer panel uses. The cap below is what makes that honest: base64 in one message is a bridge for a screenshot or a
 * small document, not a channel for a 100 MiB video, and the chunked channel is what the design's large-file case needs.
 *
 * # Why the final state is kept here
 *
 * Because the kernel removes a completed transfer from its registry — the panel's row is gone, which is right for the
 * panel and wrong for a page that asked for an id and then wants to know how it ended. `send` records the state it
 * finished in, `status` prefers that, and `onProgress` replays it to a listener that attached after the send.
 *
 * # The limitation to be honest about
 *
 * `send` resolves when the transfer is **over**, not when it starts. A page that calls `onProgress` after `await send`
 * gets the final state (replayed) but no intermediate blocks, because there are none left. Live progress goes to a
 * listener attached before the send. Making `send` return early needs a worker thread, and that is a separate change.
 */

import type { TransferNamespace, TransferProgress, TransferStatus } from '@synra/client'
import type { TransferId } from '@synra/protocol'

import type { InjectionContext } from './context'
import { KERNEL_EVENT_CHANNEL } from '../channel'
import { asSynraError, denied, tooLarge, unsupported } from './errors'

/** The largest file the chunked channel will carry, matching the kernel's `MAX_PLUGIN_UPLOAD_BYTES`. */
const MAX_TRANSFER_BYTES = 64 * 1024 * 1024

/** How much of a file one `transfer.chunk` carries, matching the kernel's `MAX_PLUGIN_CHUNK_BYTES`. */
const CHUNK_BYTES = 1024 * 1024

/** What the kernel answers for a status. */
interface RawStatus {
  readonly transferId: string
  readonly state: TransferStatus['state']
  readonly receivedBytes: number
  readonly totalBytes: number
  readonly blocksDone?: number
  readonly blocksTotal?: number
}

/** What one `transferUpdated` event carries. */
interface TransferUpdated {
  readonly type: 'transferUpdated'
  readonly payload: RawStatus
}

/** What one `transferFinished` event carries. */
interface TransferFinished {
  readonly type: 'transferFinished'
  readonly payload: { readonly transferId: string }
}

/** Whether a payload is a transfer event this namespace understands. */
function isTransferEvent(payload: unknown): payload is TransferUpdated | TransferFinished {
  if (payload === null || typeof payload !== 'object')
    return false
  const type = (payload as { type?: unknown }).type
  return type === 'transferUpdated' || type === 'transferFinished'
}

/** How far along a status is, from the bytes or — when the sender only counted blocks — from the blocks. */
function receivedOf(status: RawStatus): number {
  if (status.receivedBytes > 0 || !status.blocksTotal)
    return status.receivedBytes
  return Math.round((status.totalBytes * (status.blocksDone ?? 0)) / status.blocksTotal)
}

/** The contract's status, with the derived byte count. */
function statusOf(raw: RawStatus): TransferStatus {
  return {
    transferId: raw.transferId as TransferId,
    state: raw.state,
    receivedBytes: receivedOf(raw),
    totalBytes: raw.totalBytes
  }
}

/** The progress view of a status. */
function progressOf(status: TransferStatus): TransferProgress {
  return {
    transferId: status.transferId,
    state: status.state,
    receivedBytes: status.receivedBytes,
    totalBytes: status.totalBytes,
    ratio: status.totalBytes === 0 ? 0 : Math.min(1, status.receivedBytes / status.totalBytes)
  }
}

/** Converts a Blob to base64 without spreading a whole file into one call (which overflows the argument stack). */
async function base64Of(file: Blob): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer())
  const chunk = 0x8000
  let binary = ''
  for (let offset = 0; offset < bytes.length; offset += chunk)
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunk))
  return btoa(binary)
}

/** The transfer namespace, over the plugin channel. */
export function createTransferNamespace(context: InjectionContext): TransferNamespace {
  const granted = new Set(context.granted)
  /** The final state of the transfers this page started, and any status a read answered. */
  const known = new Map<string, TransferStatus>()

  const call = <T>(method: string, args: Readonly<Record<string, unknown>> = {}): Promise<T> =>
    context.bridge
      .invoke('plugin_ui_rpc', { token: context.token, method, args })
      .catch((reason: unknown) => {
        throw asSynraError(reason)
      }) as Promise<T>

  return {
    send: async (request): Promise<TransferId> => {
      if (!granted.has('file:transfer:send'))
        throw denied('file:transfer:send')
      if (request.contextId !== undefined)
        throw unsupported('transfer.send contextId')
      if (request.file.size > MAX_TRANSFER_BYTES)
        throw tooLarge(`the file is ${request.file.size} bytes, and the channel carries at most ${MAX_TRANSFER_BYTES}`)

      // ★ One message per chunk, not one message for the file: a `transfer.send` carrying a whole 64 MiB blob as base64
      // would be an 85 MiB IPC message, which is not a channel. The upload is aborted on any failure so the host does
      // not keep a partial file in its cache directory.
      const { uploadId } = await call<{ readonly uploadId: string }>('transfer.begin', {
        to: request.to,
        fileName: request.fileName,
        totalBytes: request.file.size
      })

      try {
        for (let offset = 0; offset < request.file.size; offset += CHUNK_BYTES) {
          const data = await base64Of(request.file.slice(offset, offset + CHUNK_BYTES))
          await call('transfer.chunk', { uploadId, data })
        }
        const answer = await call<{ readonly transferId: string }>('transfer.finish', { uploadId })
        const transferId = answer.transferId as TransferId
        known.set(answer.transferId, {
          transferId,
          state: 'completed',
          receivedBytes: request.file.size,
          totalBytes: request.file.size
        })
        return transferId
      }
      catch (reason) {
        // Best effort: the first error is the one the page needs to see, and an abort that fails says nothing it can act on.
        try {
          await call('transfer.abort', { uploadId })
        }
        catch {
          // The upload is gone either way — the host removes it on its own when it expires or the session ends.
        }
        throw reason
      }
    },

    status: async (id: TransferId): Promise<TransferStatus> => {
      const cached = known.get(id)
      if (cached !== undefined)
        return cached
      const raw = await call<RawStatus>('transfer.status', { transferId: id })
      const status = statusOf(raw)
      known.set(id, status)
      return status
    },

    onProgress: (id: TransferId, handler: (progress: TransferProgress) => void): (() => void) => {
      let active = true
      const emit = (status: TransferStatus): void => {
        if (active)
          handler(progressOf(status))
      }

      // ★ What is already known, so a listener that attached after `send` still gets an answer rather than silence.
      const cached = known.get(id)
      if (cached !== undefined)
        queueMicrotask(() => emit(cached))

      let off: (() => void) | undefined
      void context.bridge
        .listen(KERNEL_EVENT_CHANNEL, (payload: unknown) => {
          if (!active || !isTransferEvent(payload))
            return
          if (payload.type === 'transferUpdated') {
            if (payload.payload.transferId === id) {
              const status = statusOf(payload.payload)
              known.set(id, status)
              emit(status)
            }
            return
          }
          const finished = known.get(id)
          if (payload.payload.transferId === id && finished !== undefined)
            emit(finished)
        })
        .then((stop) => {
          if (active)
            off = stop
          else
            stop()
        })
        .catch(() => {
          // Nothing to report to: the page's only log is the console, and a listener that never attached is not
          // something the author can act on.
        })

      return () => {
        active = false
        off?.()
      }
    }
  }
}
