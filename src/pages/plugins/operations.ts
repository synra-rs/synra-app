import { onScopeDispose, reactive } from 'vue'

export type PluginOperation = 'consent' | 'toggle' | 'open' | 'reload' | 'check' | 'update' | 'remove' | 'syncPlan' | 'syncRun'

/** One command per plugin; independent plugins remain usable. Closing the page discards late UI results. */
export function usePluginOperations() {
  const busy = reactive(new Map<string, PluginOperation>())
  let alive = true
  onScopeDispose(() => {
    alive = false
  })

  async function run<T>(id: string, operation: PluginOperation, command: () => Promise<T>): Promise<T | undefined> {
    if (!alive || busy.has(id))
      return undefined
    busy.set(id, operation)
    try {
      const outcome = await command()
      return alive ? outcome : undefined
    }
    finally {
      busy.delete(id)
    }
  }

  return { busy, run }
}
