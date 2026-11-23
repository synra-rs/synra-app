import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { rustContract } from './rust-contract'

export const registryContractPath = fileURLToPath(new URL('../tests/fixtures/registry-contract.json', import.meta.url))
export const rustRegistryContract = (appRoot: string): unknown => rustContract(appRoot, 'registry_contract')
export function savedRegistryContract(): unknown {
  return JSON.parse(readFileSync(registryContractPath, 'utf8'))
}
