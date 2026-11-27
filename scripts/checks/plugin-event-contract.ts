import { readFileSync } from 'node:fs'
import { isDeepStrictEqual } from 'node:util'
import { rustContract } from '../rust-contract'
import { failures, here, notes } from './common'

export function checkPluginEventContract(): void {
  try {
    const fixture = new URL('../../tests/fixtures/plugin-event-contract.json', import.meta.url)
    const saved: unknown = JSON.parse(readFileSync(fixture, 'utf8'))
    if (!isDeepStrictEqual(rustContract(here, 'plugin_event_contract'), saved)) {
      failures.push({ check: 'plugin-event-contract', detail: 'Core event contract changed; review the producer and regenerate tests/fixtures/plugin-event-contract.json' })
      return
    }
    notes.push('plugin-event-contract: Rust projection and event patterns match the shared fixture')
  }
  catch (cause) {
    failures.push({ check: 'plugin-event-contract', detail: String(cause) })
  }
}
