/** Upload chunks and query the worker's transfer state through the plugin channel. */

import type { TransferNamespace, TransferProgress, TransferStatus } from '@synra/client'
import type { TransferId } from '@synra/protocol'

import type { InjectionContext } from './context'
import type { GrantCheck } from './grants'
import { KERNEL_EVENT_CHANNEL } from '../channel'
import { asSynraError, denied, tooLarge, unsupported } from './errors'

/** The largest file the chunked channel will carry, matching the kernel's `MAX_PLUGIN_UPLOAD_BYTES`. */
const MAX_TRANSFER_BYTES = 64 * 1024 * 1024

/** How much of a file one `transfer.chunk` carries, matching the kernel's `MAX_PLUGIN_CHUNK_BYTES`. */
const CHUNK_BYTES = 1024 * 1024

/** What one `transferUpdated` event carries. */
interface TransferUpdated {
  readonly type: 'transferUpdated'
  readonly payload: TransferStatus
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

/** The progress view of a status. */
function progressOf(status: TransferStatus): TransferProgress {
  return {
    ...status,
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
export function createTransferNamespace(context: InjectionContext, can: GrantCheck): TransferNamespace {
  const call = <T>(method: string, args: Readonly<Record<string, unknown>> = {}): Promise<T> =>
    context.bridge
      .invoke('plugin_ui_rpc', { token: context.token, method, args })
      .catch((reason: unknown) => {
        throw asSynraError(reason)
      }) as Promise<T>

  const status = (id: TransferId): Promise<TransferStatus> =>
    call<TransferStatus>('transfer.status', { transferId: id })

  return {
    send: async (request): Promise<TransferId> => {
      if (!can('file:transfer:send'))
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
        return answer.transferId as TransferId
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

    status,

    onProgress: (id: TransferId, handler: (progress: TransferProgress) => void): (() => void) => {
      let active = true
      let revision = -1
      const emit = (status: TransferStatus): void => {
        if (active && status.transferId === id && status.revision > revision) {
          revision = status.revision
          handler(progressOf(status))
        }
      }
      const report = (error: unknown): void => {
        if (active)
          console.error('Synra transfer progress', asSynraError(error))
      }

      let off: (() => void) | undefined
      void context.bridge
        .listen(KERNEL_EVENT_CHANNEL, (payload: unknown) => {
          if (!active || !isTransferEvent(payload))
            return
          if (payload.type === 'transferUpdated') {
            emit(payload.payload)
            return
          }
          if (payload.payload.transferId === id)
            void status(id).then(emit).catch(report)
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
      void status(id).then(emit).catch(report)

      return () => {
        active = false
        off?.()
      }
    }
  }
}
