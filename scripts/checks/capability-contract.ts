import { readFileSync } from 'node:fs'
import { isDeepStrictEqual } from 'node:util'
import { rustContract } from '../rust-contract'
import { failures, here, notes } from './common'

export function checkCapabilityContract(): void {
  try {
    const fixture = new URL('../../tests/fixtures/capability-contract.json', import.meta.url)
    const saved: unknown = JSON.parse(readFileSync(fixture, 'utf8'))
    if (!isDeepStrictEqual(rustContract(here, 'capability_contract'), saved)) {
      failures.push({ check: 'capability-contract', detail: 'Core grants changed; review the producer and regenerate tests/fixtures/capability-contract.json' })
      return
    }
    notes.push('capability-contract: Core authorization decisions match the shared conformance fixture')
  }
  catch (cause) {
    failures.push({ check: 'capability-contract', detail: String(cause) })
  }
}
