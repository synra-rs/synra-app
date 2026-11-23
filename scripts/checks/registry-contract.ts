import { isDeepStrictEqual } from 'node:util'
import { rustRegistryContract, savedRegistryContract } from '../registry-contract'
import { failures, here, notes } from './common'

export function checkRegistryContract(): void {
  try {
    if (!isDeepStrictEqual(rustRegistryContract(here), savedRegistryContract())) {
      failures.push({ check: 'registry-contract', detail: 'Rust registry serialization changed; inspect and regenerate tests/fixtures/registry-contract.json' })
      return
    }
    notes.push('registry-contract: actual Rust configuration decisions and errors match the frontend fixture')
  }
  catch (cause) {
    failures.push({ check: 'registry-contract', detail: String(cause) })
  }
}
