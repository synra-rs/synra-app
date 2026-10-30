import { computed, shallowRef, watch } from 'vue'

/** Editing buffers only; the source remains the shared store. Values here are JSON configuration fields. */
export function useSettingsDraft<T>(source: () => T) {
  const copy = (value: T): T => structuredClone(value)
  const key = (value: T): string => JSON.stringify(value)
  const baseline = shallowRef(copy(source()))
  const value = shallowRef(copy(source()))
  const dirty = computed(() => key(value.value) !== key(baseline.value))
  const conflict = computed(() => dirty.value && key(source()) !== key(baseline.value))

  function reset(): void {
    baseline.value = copy(source())
    value.value = copy(source())
  }

  watch(source, () => {
    // Unrelated config events must not erase an editing buffer. An own write can confirm its exact draft.
    if (!dirty.value || key(value.value) === key(source()))
      reset()
  })

  return { value, dirty, conflict, reset }
}
