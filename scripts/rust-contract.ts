import { execFileSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { readIpcContract } from './ipc-contract'

/** Run a real serde producer in the workspace owning the host IPC contract. */
export function rustContract(appRoot: string, example: 'error_contract' | 'registry_contract' | 'capability_contract' | 'plugin_event_contract'): unknown {
  let core = dirname(readIpcContract(appRoot).path)
  while (!existsSync(join(core, 'crates', 'synra-ipc', 'Cargo.toml'))) {
    const parent = dirname(core)
    if (parent === core)
      throw new Error('could not locate the Rust workspace owning the IPC contract')
    core = parent
  }
  const output = execFileSync('cargo', ['run', '--locked', '--quiet', '-p', 'synra-ipc', '--example', example], {
    cwd: core,
    encoding: 'utf8',
    windowsHide: true,
    timeout: 180000,
    stdio: ['ignore', 'pipe', 'pipe']
  })
  return JSON.parse(output)
}
