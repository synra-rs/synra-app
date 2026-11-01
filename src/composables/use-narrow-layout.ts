import { onMounted, onUnmounted, readonly, ref } from 'vue'

/**
 * Whether this surface is the narrow layout — `<640px`, or any pointer that is not a mouse.
 *
 * # Why the query is both a width and a pointer
 *
 * `03 §3` sets the narrow layout at `<640`, and `09 §3` raises every control to 44px where a finger is the pointer.
 * They usually agree, and they are not the same fact: a tablet at 800px has a finger and no room for a dialog, and a
 * desktop window dragged narrow has a mouse and no room either. `PairingPanel` needs "is there room for a dialog"
 * rather than either fact alone, so this asks both and takes the union.
 *
 * # Why `matchMedia` and not a resize listener
 *
 * It answers once per crossing rather than on every pixel, and it fires when a window moves to a screen with a different
 * pointer — a resize listener never hears that at all. `App.vue` reads its rail breakpoint the same way.
 */
export function useNarrowLayout() {
  const narrow = ref(false)
  let query: MediaQueryList | undefined

  function apply(event: MediaQueryList | MediaQueryListEvent): void {
    narrow.value = event.matches
  }

  onMounted(() => {
    query = window.matchMedia('(max-width: 639px), (pointer: coarse)')
    apply(query)
    query.addEventListener('change', apply)
  })

  onUnmounted(() => {
    query?.removeEventListener('change', apply)
  })

  return readonly(narrow)
}
