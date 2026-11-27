import type { TransferNamespace } from '@synra/client'
import type { PluginUploadAppend, PluginUploadBegin, PluginUploadChunk, PluginUploadEnded, PluginUploadHandle, PluginUploadOpened, PluginUploadSent } from '@synra/protocol'
import { createSynraError } from '../errors'

type UploadCall = <T>(method: string, args: Readonly<Record<string, unknown>>) => Promise<T>

/** Converts a bounded Blob without spreading a whole block into a single call. */
async function base64Of(file: Blob): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer())
  const batchBytes = 0x8000
  let binary = ''
  for (let offset = 0; offset < bytes.length; offset += batchBytes)
    binary += String.fromCharCode(...bytes.subarray(offset, offset + batchBytes))
  return btoa(binary)
}

/** Stages the source in the host-negotiated block size and returns the worker's identifier. */
export async function uploadFile(call: UploadCall, request: Parameters<TransferNamespace['send']>[0]): ReturnType<TransferNamespace['send']> {
  const { uploadId, chunkBytes } = await call<PluginUploadOpened>('transfer.begin', {
    to: request.to,
    fileName: request.fileName,
    totalBytes: request.file.size
  } satisfies PluginUploadBegin)

  try {
    if (!Number.isSafeInteger(chunkBytes) || chunkBytes <= 0)
      throw createSynraError('internal', { reason: 'invalid_upload_limit' })
    for (let offset = 0; offset < request.file.size; offset += chunkBytes) {
      const end = Math.min(offset + chunkBytes, request.file.size)
      const data = await base64Of(request.file.slice(offset, end))
      const reply = await call<PluginUploadAppend>('transfer.chunk', {
        uploadId,
        offset,
        data
      } satisfies PluginUploadChunk)
      if (reply.written !== end || reply.totalBytes !== request.file.size)
        throw createSynraError('internal', { reason: 'invalid_upload_acknowledgement' })
    }
    const answer = await call<PluginUploadSent>('transfer.finish', { uploadId } satisfies PluginUploadHandle)
    return answer.transferId
  }
  catch (failure) {
    try {
      await call<PluginUploadEnded>('transfer.abort', { uploadId } satisfies PluginUploadHandle)
    }
    catch {
      // Owner revocation and expiry reclaim the staged file if abort cannot reach the host.
    }
    throw failure
  }
}
