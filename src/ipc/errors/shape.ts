import type { SuggestedAction, SynraError } from '@synra/protocol'
import { isErrorCode } from './catalog'

const INACCESSIBLE = Symbol('inaccessible property')
const MAX_ACTIONS = 32

/** Read data properties only; accessors and arbitrary formatting hooks never run. */
export function data(value: unknown, key: string): unknown {
  if (value === null || typeof value !== 'object')
    return undefined
  try {
    const descriptor = Object.getOwnPropertyDescriptor(value, key)
    return descriptor === undefined ? undefined : 'value' in descriptor ? descriptor.value : INACCESSIBLE
  }
  catch {
    return INACCESSIBLE
  }
}

const ACTION_FIELDS = {
  openSettings: 'section',
  requestCapability: 'capability',
  installPlugin: 'pluginId',
  connectDevice: 'deviceId',
  retry: null,
  viewLogs: 'traceId'
} satisfies Record<SuggestedAction['kind'], string | null>

function action(value: unknown): SuggestedAction | undefined {
  const kind = data(value, 'kind')
  if (typeof kind !== 'string' || !Object.hasOwn(ACTION_FIELDS, kind))
    return undefined
  const field = ACTION_FIELDS[kind as SuggestedAction['kind']]
  if (field === null)
    return { kind: 'retry' }
  const text = data(value, field)
  if (typeof text !== 'string')
    return undefined
  switch (kind) {
    case 'openSettings': return { kind, section: text }
    case 'requestCapability': return { kind, capability: text }
    case 'installPlugin': return { kind, pluginId: text }
    case 'connectDevice': return { kind, deviceId: text }
    case 'viewLogs': return { kind, traceId: text }
  }
}

/** Recognize an error-shaped response even when its code or optional fields violate the contract. */
export function hasErrorFields(value: unknown): boolean {
  return typeof data(value, 'code') === 'string'
    && typeof data(value, 'message') === 'string'
    && typeof data(value, 'retryable') === 'boolean'
}

export interface ErrorSnapshot extends SynraError {
  readonly params?: unknown
}

/** Capture each untrusted property once so normalization cannot invalidate a prior check. */
export function errorSnapshot(value: unknown): ErrorSnapshot | undefined {
  try {
    if (Array.isArray(value))
      return undefined
    const code = data(value, 'code')
    const message = data(value, 'message')
    const retryable = data(value, 'retryable')
    if (!isErrorCode(code) || typeof message !== 'string' || typeof retryable !== 'boolean') {
      return undefined
    }
    const traceId = data(value, 'traceId')
    if (traceId !== undefined && typeof traceId !== 'string')
      return undefined
    const details = data(value, 'details')
    const params = data(value, 'params')
    if (details === INACCESSIBLE || params === INACCESSIBLE)
      return undefined
    const actions = data(value, 'actions')
    const copied: SuggestedAction[] = []
    if (actions !== undefined) {
      const length = data(actions, 'length')
      if (!Array.isArray(actions) || typeof length !== 'number' || !Number.isInteger(length)
        || length < 0 || length > MAX_ACTIONS) {
        return undefined
      }
      for (let index = 0; index < length; index++) {
        const item = action(data(actions, String(index)))
        if (item === undefined)
          return undefined
        copied.push(item)
      }
    }
    return {
      code,
      message,
      retryable,
      ...(traceId === undefined ? {} : { traceId }),
      ...(details === undefined ? {} : { details }),
      ...(params === undefined ? {} : { params }),
      ...(actions === undefined ? {} : { actions: copied })
    }
  }
  catch {
    return undefined
  }
}

export function isSynraErrorValue(value: unknown): value is SynraError {
  return errorSnapshot(value) !== undefined
}
