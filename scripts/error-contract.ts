import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { readIpcContract } from './ipc-contract'

export const errorContractPath = fileURLToPath(new URL('../tests/fixtures/error-contract.json', import.meta.url))

/** Execute the real serde producer; do not reproduce Rust's wire shape in a TypeScript fixture builder. */
export function rustErrorContract(appRoot: string): unknown {
  let core = dirname(readIpcContract(appRoot).path)
  while (!existsSync(join(core, 'crates', 'synra-ipc', 'Cargo.toml'))) {
    const parent = dirname(core)
    if (parent === core)
      throw new Error('could not locate the Rust workspace owning the IPC contract')
    core = parent
  }
  const output = execFileSync('cargo', ['run', '--locked', '--quiet', '-p', 'synra-ipc', '--example', 'error_contract'], {
    cwd: core,
    encoding: 'utf8',
    windowsHide: true,
    timeout: 180000,
    stdio: ['ignore', 'pipe', 'pipe']
  })
  return JSON.parse(output)
}

export function savedErrorContract(): unknown {
  return JSON.parse(readFileSync(errorContractPath, 'utf8'))
}
