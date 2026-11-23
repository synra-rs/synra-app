import { writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { errorContractPath, rustErrorContract } from './error-contract'

const here = fileURLToPath(new URL('..', import.meta.url))
writeFileSync(errorContractPath, `${JSON.stringify(rustErrorContract(here), null, 2)}\n`)
