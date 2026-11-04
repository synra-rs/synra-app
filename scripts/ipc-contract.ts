import { existsSync, readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import process from 'node:process'

/** Read the kernel contract from a local sibling, a CI checkout, or an explicit root. */
export function readIpcContract(appRoot: string, coreRoot: string | undefined = process.env.SYNRA_CORE_ROOT): { path: string, source: string } {
  const roots = [coreRoot, resolve(appRoot, '..', 'synra-rust-core'), resolve(appRoot, 'synra-rust-core')]
  const candidates = roots
    .filter((root): root is string => root !== undefined && root !== '')
    .map(root => resolve(root, 'crates', 'synra-ipc', 'src', 'command.rs'))
  const path = candidates.find(candidate => existsSync(candidate))
  if (path === undefined) {
    throw new Error(
      'the IPC contract is missing — check out synra-rust-core beside or inside the app, '
      + `or set SYNRA_CORE_ROOT to the kernel checkout. Looked for:\n${candidates.join('\n')}`
    )
  }
  try {
    return { path, source: readFileSync(path, 'utf8') }
  }
  catch (cause) {
    throw new Error(`the IPC contract is unreadable at ${path}`, { cause })
  }
}
