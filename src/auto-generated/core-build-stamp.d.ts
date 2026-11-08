import type { BuildStamp } from 'unplugin-build-stamp/client'

declare global {
  const __CORE_BUILD_STAMP__: BuildStamp
}

export {}
