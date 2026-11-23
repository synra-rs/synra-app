import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { rustContract } from './rust-contract'

export const errorContractPath = fileURLToPath(new URL('../tests/fixtures/error-contract.json', import.meta.url))

/** Execute the real serde producer; do not reproduce Rust's wire shape in a TypeScript fixture builder. */
export function rustErrorContract(appRoot: string): unknown {
  return rustContract(appRoot, 'error_contract')
}

export function savedErrorContract(): unknown {
  return JSON.parse(readFileSync(errorContractPath, 'utf8'))
}
