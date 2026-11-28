import type { PluginTransferFailure, PluginTransferStatus, TransferAbortReason } from '@synra/protocol'
import { createSynraError } from '../errors'
import { data } from '../errors/shape'

const ABORT_REASONS: ReadonlySet<string> = new Set<TransferAbortReason>([
  'user_cancelled',
  'digest_mismatch',
  'too_large',
  'io_error',
  'idle_timeout',
  'total_timeout',
  'peer_rejected'
])

function invalid(): never {
  throw createSynraError('protocol_violation', { reason: 'invalid plugin transfer status' })
}

function count(raw: unknown, key: string): number {
  const value = data(raw, key)
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0)
    return invalid()
  return value
}

function abortReason(value: unknown): value is TransferAbortReason {
  return typeof value === 'string' && ABORT_REASONS.has(value)
}

function failureOf(raw: unknown): PluginTransferFailure {
  const kind = data(raw, 'kind')
  switch (kind) {
    case 'connection':
    case 'timed_out':
    case 'authorization':
    case 'file_io':
    case 'integrity':
    case 'protocol':
    case 'worker_unavailable':
      return { kind }
    case 'peer_aborted': {
      const reason = data(raw, 'reason')
      if (!abortReason(reason))
        return invalid()
      return { kind, reason }
    }
    default:
      return invalid()
  }
}

/** Build the same public snapshot from a query or a projected event. */
export function decodeTransferStatus(raw: unknown): PluginTransferStatus {
  const transferId = data(raw, 'transferId')
  const state = data(raw, 'state')
  if (typeof transferId !== 'string' || transferId.length === 0
    || (state !== 'pending' && state !== 'transferring' && state !== 'completed' && state !== 'failed' && state !== 'cancelled')) {
    return invalid()
  }
  const receivedBytes = count(raw, 'receivedBytes')
  const totalBytes = count(raw, 'totalBytes')
  const blocksDone = count(raw, 'blocksDone')
  const blocksTotal = count(raw, 'blocksTotal')
  const revision = count(raw, 'revision')
  if (receivedBytes > totalBytes || blocksDone > blocksTotal)
    return invalid()
  const status: PluginTransferStatus = {
    transferId,
    state,
    receivedBytes,
    totalBytes,
    blocksDone,
    blocksTotal,
    revision
  }
  if (state === 'failed') {
    return {
      ...status,
      endedAtMs: count(raw, 'endedAtMs'),
      failure: failureOf(data(raw, 'failure'))
    }
  }
  if (state === 'completed' || state === 'cancelled') {
    return {
      ...status,
      endedAtMs: count(raw, 'endedAtMs')
    }
  }
  return status
}
