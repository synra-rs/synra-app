import type { Ref } from 'vue'
import type { Router } from 'vue-router'
import type { HostEnvironment } from './host-environment'
import { watch } from 'vue'
import { dismissActiveDialog } from '../stores/dialogs'
import { createHostBackHandler } from './back'
import { subscribeWhileActive } from './native-listener'

export function useHostBack(environment: Readonly<Ref<HostEnvironment | undefined>>, router: Router, prompt: () => void): void {
  const report = (error: unknown): void => console.warn('Synra Android back', error)
  const android = (): boolean => environment.value?.kind === 'mobile' && environment.value.platform === 'android'

  watch(android, (enabled, _, onCleanup) => {
    if (!enabled)
      return
    onCleanup(subscribeWhileActive(async (isActive) => {
      const { exit, onBackButtonPress } = await import('@tauri-apps/api/app')
      const handle = createHostBackHandler({
        dismissDialog: dismissActiveDialog,
        currentRoute: () => router.currentRoute.value,
        hasRouterHistory: () => typeof router.options.history.state.back === 'string',
        goBack: () => router.back(),
        replace: path => router.replace(path),
        prompt,
        exit: () => exit(0)
      })
      const listener = await onBackButtonPress(({ canGoBack }) => {
        if (isActive())
          void handle(canGoBack).catch(report)
      })
      return () => listener.unregister()
    }, report))
  }, { immediate: true })
}
