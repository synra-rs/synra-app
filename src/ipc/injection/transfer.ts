/** Upload chunks and query the worker's transfer state through the plugin channel. */

import type { TransferNamespace, TransferProgress, TransferStatus } from '@synra/client'
import type { TransferId } from '@synra/protocol'

import type { InjectionContext } from './context'
import type { GrantCheck } from './grants'
import { KERNEL_EVENT_CHANNEL } from '../channel'
import { asSynraError, denied, unsupported } from './errors'
import { uploadFile } from './transfer-upload'

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
      return uploadFile(call, request)
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
