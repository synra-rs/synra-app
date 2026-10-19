import type { Ref } from 'vue'
import { onBeforeUnmount, onMounted, ref } from 'vue'

/**
 * Whether this machine currently has a network.
 *
 * # Why the browser's flag rather than a kernel probe
 *
 * `navigator.onLine` is what the user's operating system believes, and the question the banner answers is "should I
 * expect sharing to work right now", not "can I reach a particular peer". A kernel probe per page would bind sockets to
 * answer a question the platform already answers for free, and would still be wrong about a peer that is simply off.
 *
 * # Why the listener is removed
 *
 * An `online`/`offline` listener on `window` outlives the component that added it. The shell is the one component that
 * lives as long as the window, but removing it is still the contract: a composable that leaks a listener is one that
 * cannot be used twice.
 */
export function useNetworkStatus(): Ref<boolean> {
  const online = ref(typeof navigator === 'undefined' ? true : navigator.onLine)

  function update(): void {
    online.value = typeof navigator === 'undefined' ? true : navigator.onLine
  }

  onMounted(() => {
    window.addEventListener('online', update)
    window.addEventListener('offline', update)
    update()
  })

  onBeforeUnmount(() => {
    window.removeEventListener('online', update)
    window.removeEventListener('offline', update)
  })

  return online
}
