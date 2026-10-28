import type { ConfigSnapshot } from '@synra/protocol'

type StorageLike = Pick<Storage, 'getItem' | 'setItem'>
const KEY = 'synra.preview.config'

function record(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function browserStorage(): StorageLike | undefined {
  try {
    return typeof localStorage === 'undefined' ? undefined : localStorage
  }
  catch {
    return undefined
  }
}

/** Browser-demo preferences only. The desktop always uses the kernel's configuration. */
export class PreviewConfig {
  readonly #storage: StorageLike | undefined
  #values: Record<string, unknown> = {}

  constructor(storage: StorageLike | undefined = browserStorage()) {
    this.#storage = storage
    try {
      const saved: unknown = JSON.parse(storage?.getItem(KEY) ?? '{}')
      if (record(saved))
        this.#values = saved
    }
    catch {
      // A corrupt demo preference must not prevent the preview from opening.
    }
  }

  snapshot(): ConfigSnapshot {
    return { values: structuredClone(this.#values), overridden: Object.keys(this.#values) }
  }

  apply(patch: unknown): ConfigSnapshot | 'invalid' | 'unavailable' {
    if (!record(patch) || (patch.set !== undefined && !record(patch.set))
      || (patch.unset !== undefined && (!Array.isArray(patch.unset) || !patch.unset.every(key => typeof key === 'string')))) {
      return 'invalid'
    }
    const next = { ...this.#values, ...(patch.set as Record<string, unknown> | undefined) }
    for (const key of (patch.unset ?? []) as string[])
      delete next[key]
    try {
      const serialized = JSON.stringify(next)
      this.#storage?.setItem(KEY, serialized)
      this.#values = JSON.parse(serialized) as Record<string, unknown>
      return this.snapshot()
    }
    catch {
      return 'unavailable'
    }
  }
}
