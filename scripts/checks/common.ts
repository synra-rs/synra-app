import { readdir } from 'node:fs/promises'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

export const here = fileURLToPath(new URL('../..', import.meta.url))

export const clientTypes = join(here, '..', 'synra-js', 'packages', 'protocol', 'src')

export interface Failure {
  readonly check: string
  readonly detail: string
}

export const failures: Failure[] = []

export const notes: string[] = []

export async function sources(directory: string): Promise<string[]> {
  const out: string[] = []
  const walk = async (current: string): Promise<void> => {
    let entries
    try {
      entries = await readdir(current, { withFileTypes: true })
    }
    catch {
      return
    }
    for (const entry of entries) {
      const path = join(current, entry.name)
      if (entry.isDirectory()) {
        if (entry.name === 'node_modules' || entry.name === 'dist')
          continue
        await walk(path)
      }
      else if (entry.name.endsWith('.ts') || entry.name.endsWith('.vue')) {
        out.push(path)
      }
    }
  }
  await walk(directory)
  return out
}
