/** A primary page has no nested query or hash state left to leave first. */
export function isPrimaryRoute(path: string, query: Readonly<Record<string, unknown>>, hash: string): boolean {
  return (path === '/' || path === '/devices' || path === '/plugins' || path === '/settings')
    && Object.keys(query).length === 0
    && hash === ''
}

export const EXIT_CONFIRMATION_MS = 1800

interface BackControls {
  dismissDialog: () => boolean
  currentRoute: () => { readonly path: string, readonly query: Readonly<Record<string, unknown>>, readonly hash: string }
  hasRouterHistory: () => boolean
  goBack: () => void
  replace: (path: string) => Promise<unknown>
  prompt: () => void
  exit: () => Promise<void>
}

export function createHostBackHandler(controls: BackControls, nowMs: () => number = () => performance.now()) {
  let armedAt: number | undefined
  let pending = false

  return async (canGoBack: boolean): Promise<void> => {
    if (pending)
      return
    if (controls.dismissDialog()) {
      armedAt = undefined
      return
    }
    if (canGoBack && controls.hasRouterHistory()) {
      armedAt = undefined
      controls.goBack()
      return
    }

    let transition: () => Promise<unknown>
    const { path, query, hash } = controls.currentRoute()
    if (!isPrimaryRoute(path, query, hash)) {
      const destination = isPrimaryRoute(path, {}, '')
        ? path
        : '/'
      transition = () => controls.replace(destination)
    }
    else {
      const now = nowMs()
      if (armedAt === undefined || now - armedAt >= EXIT_CONFIRMATION_MS) {
        armedAt = now
        controls.prompt()
        return
      }
      transition = controls.exit
    }
    armedAt = undefined
    pending = true
    try {
      await transition()
    }
    finally {
      pending = false
    }
  }
}
