/** Upload chunks and query the worker's transfer state through the plugin channel. */

import type { TransferNamespace, TransferProgress, TransferStatus } from '@synra/client'
import type { TransferId } from '@synra/protocol'

import type { InjectionContext } from './context'
import type { PageSubscriptions } from './event/subscriptions'
import type { GrantCheck } from './grants'
import { asSynraError, denied, unsupported } from './errors'
import { decodeTransferStatus } from './transfer-status'
import { uploadFile } from './transfer-upload'

/** The progress view of a status. */
function progressOf(status: TransferStatus): TransferProgress {
  return {
    ...status,
    ratio: status.totalBytes === 0
      ? 0
      : Math.min(1, status.receivedBytes / status.totalBytes)
  }
}

/** The transfer namespace, over the plugin channel. */
export function createTransferNamespace(context: InjectionContext, can: GrantCheck, subscriptions: PageSubscriptions): TransferNamespace {
  const call = <T>(method: string, args: Readonly<Record<string, unknown>> = {}): Promise<T> =>
    context.bridge
      .invoke('plugin_ui_rpc', { token: context.token, method, args })
      .catch((reason: unknown) => {
        throw asSynraError(reason)
      }) as Promise<T>

  const status = (id: TransferId): Promise<TransferStatus> =>
    call<unknown>('transfer.status', { transferId: id }).then(decodeTransferStatus)

  return {
    send: async (request): Promise<TransferId> => {
      if (!can('file:transfer:send'))
        throw denied('file:transfer:send')
      if (request.contextId !== undefined)
        throw unsupported('transfer.send contextId')
      return uploadFile(call, request)
    },

    status,

    onProgress: (id, handler, options) => {
      let revision = -1
      const emit = (status: TransferStatus): void => {
        if (status.transferId === id && status.revision > revision) {
          revision = status.revision
          handler(progressOf(status))
        }
      }
      return subscriptions.subscribeTransfer(id, emit, () => status(id), options)
    }
  }
}
