type Unlisten = () => void | Promise<void>

/** Dispose a native listener even when registration finishes after the component leaves. */
export function subscribeWhileActive(
  register: (isActive: () => boolean) => Promise<Unlisten>,
  report: (error: unknown) => void
): () => void {
  let active = true
  let unlisten: Unlisten | undefined

  async function attach(): Promise<void> {
    const registered = await register(() => active)
    if (active)
      unlisten = registered
    else
      await registered()
  }
  async function detach(): Promise<void> {
    const registered = unlisten
    unlisten = undefined
    await registered?.()
  }

  void attach().catch(report)
  return () => {
    active = false
    void detach().catch(report)
  }
}
