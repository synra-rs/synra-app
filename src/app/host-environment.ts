import { hasTauriInternals } from '../ipc/tauri'

export type HostEnvironment
  = { readonly kind: 'browser' }
    | { readonly kind: 'desktop', readonly platform: 'windows' | 'macos' | 'linux' }
    | { readonly kind: 'mobile', readonly platform: 'android' | 'ios' }

/** Native platform values come from the local device snapshot. */
export function hostEnvironment(shell: unknown, platform: unknown): HostEnvironment | undefined {
  if (!hasTauriInternals(shell))
    return { kind: 'browser' }

  switch (platform) {
    case 'windows':
    case 'macos':
    case 'linux':
      return { kind: 'desktop', platform }
    case 'android':
    case 'ios':
      return { kind: 'mobile', platform }
    default:
      return undefined
  }
}
