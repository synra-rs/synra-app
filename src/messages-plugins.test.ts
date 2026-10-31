/**
 * The four health rungs, and the fifth that does not exist yet.
 *
 * ★ `synra-plugin-host`'s `Health` is `#[non_exhaustive]`, so a rung added there arrives as a string this build has
 * never seen. The label chain used to end in "Running normally" — the worst possible default, because it reports the
 * healthiest rung for a state nobody examined. These tests pin the fallback to something that says it does not know.
 *
 * **Both languages, because `messages` is a Proxy over the *selected* catalog.** A check written against whatever
 * language the test host happens to resolve passes while the other catalog keeps the bug — which is exactly what
 * happened on the first attempt: the fix landed in English, the host resolved `zh-CN`, and a deliberately broken
 * English table still went green.
 */
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { setLocalePreference } from './locale'
import { messages } from './messages'

/** The strings `Health::as_str` produces, in `budget.rs`. */
const HEALTH_RUNGS = ['good', 'overreaching', 'degraded', 'faulted'] as const

beforeEach(() => {
  setLocalePreference('en')
})

afterEach(() => {
  setLocalePreference('system')
})

describe.each(['en', 'zh-CN'] as const)('plugin health labels (%s)', (language) => {
  beforeEach(() => {
    setLocalePreference(language)
  })

  it.each(HEALTH_RUNGS)('names the rung %s', (rung) => {
    expect(messages.plugins.healthLabel(rung)).toBeTypeOf('string')
    expect(messages.plugins.healthLabel(rung).length).toBeGreaterThan(0)
  })

  it('says so about a rung this build does not know', () => {
    // The rung named here is not in `Health`; it stands for whatever a newer kernel sends.
    const unknown = messages.plugins.healthLabel('throttled')
    expect(unknown).not.toBe(messages.plugins.healthLabel('good'))
  })

  it('gives each known rung its own sentence', () => {
    // A rung that shares words with another is a rung the reader cannot tell from it.
    const sentences = HEALTH_RUNGS.map(rung => messages.plugins.healthLabel(rung))
    expect(new Set(sentences).size).toBe(HEALTH_RUNGS.length)
  })
})
