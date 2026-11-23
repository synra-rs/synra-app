import { writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { registryContractPath, rustRegistryContract } from './registry-contract'

const here = fileURLToPath(new URL('..', import.meta.url))
writeFileSync(registryContractPath, `${JSON.stringify(rustRegistryContract(here), null, 2)}\n`)
