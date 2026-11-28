import { isDeepStrictEqual } from 'node:util'
import { rustErrorContract, savedErrorContract } from '../error-contract'
import { failures, here, notes } from './common'

export function checkErrorContract(): void {
  try {
    if (!isDeepStrictEqual(rustErrorContract(here), savedErrorContract())) {
      failures.push({ check: 'error-contract', detail: 'Rust error serialization changed; inspect and regenerate tests/fixtures/error-contract.json' })
      return
    }
    notes.push('error-contract: actual Rust serde output matches the frontend conformance fixture')
  }
  catch (cause) {
    failures.push({
      check: 'error-contract',
      detail: String(cause)
    })
  }
}
