/// <reference types="unplugin-build-stamp/client" />

/** The frontend build identity, replaced with literals by Vite at build time. */
export const buildStamp = __BUILD_STAMP__

/** The core build identity, replaced by the second build-stamp plugin at build time. */
export const coreBuildStamp = __CORE_BUILD_STAMP__
