import type { PluginSnapshot } from '@synra/protocol'
import { messages } from '../../messages'
import { formatBytes } from '../../utils/format'

function quotaNumber(health: PluginSnapshot['health'], key: string): number | undefined {
  if (health === undefined)
    return undefined
  const usage = health.usage
  if (typeof usage !== 'object' || usage === null)
    return undefined
  const value = (usage as Record<string, unknown>)[key]
  return typeof value === 'number' ? value : undefined
}

/** Every quota field the report carries, for the disclosure under the summary row. */
export function quotaRows(health: PluginSnapshot['health']): { label: string, value: string }[] {
  const rows: { label: string, value: string }[] = []
  const add = (key: string, label: string, format: (value: number) => string = String): void => {
    const value = quotaNumber(health, key)
    if (value) {
      rows.push({ label, value: format(value) })
    }
  }
  add('maxMemoryBytes', messages.plugins.limitMemory, formatBytes)
  add('maxInstances', messages.plugins.limitInstances)
  add('maxStorageBytes', messages.plugins.limitStorage, formatBytes)
  add('maxOpenTransfers', messages.plugins.limitTransfers)
  add('hostCallsPerSec', messages.plugins.limitCalls)
  add('hostCallsBurst', messages.plugins.limitBurst)
  add('eventsPerSec', messages.plugins.limitEvents)
  add('networkRequestsPerSec', messages.plugins.limitNetwork)
  add('networkConcurrent', messages.plugins.limitConcurrent)
  add('deviceSendsPerSec', messages.plugins.limitSends)
  add('fsOpsPerSec', messages.plugins.limitFsOps)
  add('callFuel', messages.plugins.limitFuel)
  add('callTimeoutMs', messages.plugins.limitCallTime, value => `${value / 1000} s`)
  return rows
}
