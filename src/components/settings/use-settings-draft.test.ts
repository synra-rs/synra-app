// @vitest-environment jsdom
/**
 * The settings draft buffer.
 *
 * # Why this hook has its own tests
 *
 * The settings page exercises the happy path through its mounts, but the rule that matters most here is a negative one:
 * **an unrelated config event must not erase what the user is typing**. A page-level test proves the conflict sentence
 * appears; it does not prove the buffer survived, because the page re-renders from the same ref either way.
 */
import { describe, expect, it } from 'vitest'
import { nextTick, reactive, toRaw } from 'vue'

import { useSettingsDraft } from './use-settings-draft'

/**
 * A stand-in for the shared settings store.
 *
 * `toRaw` because the hook uses `structuredClone`, which refuses a Vue proxy — the real store hands out plain objects
 * (a snapshot from the kernel), so this only matches what the page does in practice.
 */
function source<T extends object>(initial: T) {
  const state = reactive({ value: initial }) as { value: T }
  return { state, read: () => toRaw(state.value) as T }
}

describe('use settings draft', () => {
  it('starts clean and is not in conflict', () => {
    const s = source({ name: 'a' })
    const draft = useSettingsDraft(s.read)
    expect(draft.dirty.value).toBe(false)
    expect(draft.conflict.value).toBe(false)
    expect(draft.value.value).toEqual({ name: 'a' })
  })

  it('marks itself dirty when the buffer differs, without touching the source', () => {
    const s = source({ name: 'a' })
    const draft = useSettingsDraft(s.read)
    draft.value.value = { name: 'b' }
    expect(draft.dirty.value).toBe(true)
    // Still no conflict: the source has not moved, only the buffer.
    expect(draft.conflict.value).toBe(false)
    expect(s.state.value).toEqual({ name: 'a' })
  })

  it('reports a conflict when the source moves under an edited buffer', () => {
    const s = source({ name: 'a' })
    const draft = useSettingsDraft(s.read)
    draft.value.value = { name: 'b' }
    s.state.value = { name: 'c' }
    expect(draft.conflict.value).toBe(true)
  })

  it('keeps the draft when an unrelated config event arrives', async () => {
    // ★ The rule this file exists for. Someone else's write reaches the store while this field is being edited; the
    // buffer must survive it, or the user's typing disappears for a change they did not make.
    const s = source({ name: 'a' })
    const draft = useSettingsDraft(s.read)
    draft.value.value = { name: 'b' }

    s.state.value = { name: 'c' }
    await nextTick()

    expect(draft.value.value).toEqual({ name: 'b' })
    expect(draft.dirty.value).toBe(true)
  })

  it('adopts an external change once the buffer is clean again', async () => {
    // The other half: with nothing being edited, the buffer follows the store. Without this the page would show a stale
    // value after someone else's change.
    const s = source({ name: 'a' })
    const draft = useSettingsDraft(s.read)

    s.state.value = { name: 'c' }
    await nextTick()

    expect(draft.value.value).toEqual({ name: 'c' })
    expect(draft.dirty.value).toBe(false)
  })

  it('adopts its own write when the store lands on exactly what was typed', async () => {
    // The exception in the `watch`: a write that agrees with the draft confirms it rather than leaving the field dirty
    // for ever.
    const s = source({ name: 'a' })
    const draft = useSettingsDraft(s.read)
    draft.value.value = { name: 'b' }

    s.state.value = { name: 'b' }
    await nextTick()

    expect(draft.dirty.value).toBe(false)
    expect(draft.conflict.value).toBe(false)
  })

  it('drops both buffer and conflict on reset', () => {
    const s = source({ name: 'a' })
    const draft = useSettingsDraft(s.read)
    draft.value.value = { name: 'b' }
    s.state.value = { name: 'c' }
    expect(draft.conflict.value).toBe(true)

    draft.reset()
    expect(draft.value.value).toEqual({ name: 'c' })
    expect(draft.dirty.value).toBe(false)
    expect(draft.conflict.value).toBe(false)
  })

  it('edits a copy, not the object in the store', () => {
    // `copy` is `structuredClone`, and the source object is passed by reference. Without the clone, editing the buffer
    // would edit the store's object directly and `dirty` would never go true.
    const s = source({ nested: { name: 'a' } })
    const draft = useSettingsDraft(s.read)
    expect(draft.value.value).not.toBe(s.state.value)
    expect(draft.value.value.nested).not.toBe(s.state.value.nested)
  })
})
