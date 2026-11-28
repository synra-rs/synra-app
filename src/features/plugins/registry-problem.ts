import type { RegistryProblem } from '@synra/protocol'
import { data } from '../../ipc/errors/shape'

/** Registry workflow decisions belong to their typed envelope, never to translated prose. */
export function registryProblem(details: unknown): RegistryProblem | undefined {
  const value = data(details, 'registry')
  const kind = data(value, 'kind')
  if (kind === 'notConfigured' || kind === 'searchUnsupported')
    return { kind }
  if (kind !== 'invalidConfiguration')
    return undefined
  const field = data(value, 'field')
  const reason = data(value, 'reason')
  const index = data(value, 'index')
  if (typeof field !== 'string' || typeof reason !== 'string'
    || (index !== undefined && (typeof index !== 'number' || !Number.isSafeInteger(index) || index < 0))) {
    return undefined
  }
  return {
    kind,
    field,
    reason,
    ...(index === undefined
      ? {}
      : { index })
  }
}

export function registryDetail(problem: RegistryProblem | undefined): string | undefined {
  if (problem?.kind !== 'invalidConfiguration')
    return undefined
  const field = problem.index === undefined
    ? problem.field
    : `${problem.field}[${problem.index}]`
  return `${field}: ${problem.reason}`
}
