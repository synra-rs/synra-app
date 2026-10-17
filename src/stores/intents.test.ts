/**
 * What the intents store does with the four events it knows.
 *
 * The interesting cases are not "it appends": they are that a second `intentUpdated` **replaces in place** (so the row
 * the user is reading does not jump), that a `*Finished` **removes** rather than marking, and that an event about a
 * different kind is refused so its caller can re-read instead of assuming nothing happened.
 */

import type { IntentSnapshot, TransferSnapshot } from '@synra/protocol'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'

import { useIntentsStore } from './intents'

function intent(intentId: string, overrides: Partial<IntentSnapshot> = {}): IntentSnapshot {
  return { intentId, summary: '', state: 'awaiting-choice', candidates: [], ...overrides } as IntentSnapshot
}

function transfer(transferId: string, overrides: Partial<TransferSnapshot> = {}): TransferSnapshot {
  return { transferId, direction: 'send', state: 'active', ...overrides } as TransferSnapshot
}

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('the intents store', () => {
  it('applies an arrival and then a replacement in place', async () => {
    const store = useIntentsStore()

    expect(store.apply({ type: 'intentUpdated', payload: intent('a') })).toBe(true)
    expect(store.apply({ type: 'intentUpdated', payload: intent('b') })).toBe(true)
    expect(store.apply({ type: 'intentUpdated', payload: intent('a', { state: 'delivering' }) })).toBe(true)

    expect(store.intents.map(i => i.intentId)).toEqual(['a', 'b'])
    expect(store.intent('a')?.state).toBe('delivering')
  })

  it('removes a finished intent', async () => {
    const store = useIntentsStore()
    store.apply({ type: 'intentUpdated', payload: intent('a') })
    store.apply({ type: 'intentUpdated', payload: intent('b') })

    store.apply({ type: 'intentFinished', payload: { intentId: 'a' } })

    expect(store.intents.map(i => i.intentId)).toEqual(['b'])
  })

  it('keeps transfers apart from intents', async () => {
    const store = useIntentsStore()

    store.apply({ type: 'transferUpdated', payload: transfer('t1') })

    expect(store.transfers.map(t => t.transferId)).toEqual(['t1'])
    expect(store.intents).toHaveLength(0)
  })

  it('drops a finished transfer', async () => {
    const store = useIntentsStore()
    store.apply({ type: 'transferUpdated', payload: transfer('t1') })

    store.apply({ type: 'transferFinished', payload: { transferId: 't1' } })

    expect(store.transfers).toHaveLength(0)
  })

  it('refuses an event about something else, so the caller re-reads', () => {
    // ★ The same contract the device store has: `false` is not "nothing happened", it is "I cannot place this".
    const store = useIntentsStore()

    expect(store.apply({ type: 'deviceCardUpdated', payload: {} } as never)).toBe(false)
  })
})
