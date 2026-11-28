import { onMounted, onUnmounted, shallowRef } from 'vue'

const SIDEBAR_BREAKPOINT = '(min-width: 48rem)'
const WIDE_BREAKPOINT = '(min-width: 64rem)'

type ViewportLayout
  = { readonly size: 'compact', readonly navigation: 'bottom' | 'rail' }
    | { readonly size: 'wide', readonly navigation: 'sidebar' }

export function useViewportLayout() {
  const layout = shallowRef<ViewportLayout>({ size: 'wide', navigation: 'sidebar' })
  let stop: (() => void) | undefined

  onMounted(() => {
    if (typeof window.matchMedia !== 'function')
      return
    const sidebar = window.matchMedia(SIDEBAR_BREAKPOINT)
    const wide = window.matchMedia(WIDE_BREAKPOINT)
    const refresh = (): void => {
      layout.value = wide.matches
        ? { size: 'wide', navigation: 'sidebar' }
        : {
            size: 'compact',
            navigation: sidebar.matches ? 'rail' : 'bottom'
          }
    }
    refresh()
    sidebar.addEventListener('change', refresh)
    wide.addEventListener('change', refresh)
    stop = () => {
      sidebar.removeEventListener('change', refresh)
      wide.removeEventListener('change', refresh)
    }
  })
  onUnmounted(() => stop?.())
  return layout
}
