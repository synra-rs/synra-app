import type { ErrorCode, IpcError, SynraError } from '@synra/protocol'
import type { ErrorSnapshot } from './shape'
import { ERROR_RETRYABLE } from './catalog'
import { diagnostic, diagnosticText } from './diagnostics'
import { data, errorSnapshot } from './shape'

export { isSynraErrorValue } from './shape'

export function createSynraError(code: ErrorCode, details?: unknown): SynraError {
  return {
    code,
    message: `error.${code}`,
    retryable: ERROR_RETRYABLE[code],
    ...(details === undefined
      ? {}
      : {
          details: diagnostic(details)
        })
  }
}

/** IPC parameters remain diagnostic and cannot override the public classification. */
export function asSynraError(reason: unknown): SynraError {
  const snapshot = errorSnapshot(reason)
  if (snapshot === undefined)
    return unknownError(reason)
  return fromSnapshot(snapshot)
}

function unknownError(reason: unknown): SynraError {
  const message = data(reason, 'message')
  const detail = typeof reason === 'string'
    ? reason
    : typeof message === 'string'
      ? message
      : 'unrecognized bridge failure'
  return createSynraError('internal', { detail })
}

function fromSnapshot(snapshot: ErrorSnapshot): SynraError {
  const { code, message, retryable, params, details: context, actions, traceId } = snapshot
  const details = params === undefined
    ? context
    : { params, context }
  return {
    code,
    message: diagnosticText(message),
    retryable,
    ...(actions === undefined
      ? {}
      : { actions }),
    ...(traceId === undefined
      ? {}
      : {
          traceId: diagnosticText(traceId)
        }),
    ...(details === undefined
      ? {}
      : {
          details: diagnostic(details)
        })
  }
}

/** Preserve the IPC shape for host readers while using the same validation and diagnostic policy. */
export function asIpcError(reason: unknown): IpcError {
  const snapshot = errorSnapshot(reason)
  const normalized = snapshot === undefined
    ? unknownError(reason)
    : fromSnapshot(snapshot)
  const params = snapshot === undefined
    ? normalized.details
    : snapshot.params
  const details = snapshot?.details
  return {
    code: normalized.code,
    message: snapshot === undefined
      ? 'error.bridgeFailed'
      : normalized.message,
    retryable: normalized.retryable,
    ...(normalized.actions === undefined
      ? {}
      : { actions: normalized.actions }),
    ...(params === undefined
      ? {}
      : {
          params: diagnostic(params)
        }),
    ...(details === undefined
      ? {}
      : {
          details: diagnostic(details)
        })
  }
}
