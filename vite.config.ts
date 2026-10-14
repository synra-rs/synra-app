import { fileURLToPath } from 'node:url'

import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vite'

/**
 * The host interface's build.
 *
 * `@synra/client` is a **type-only** package: it has no runtime entry, which is why the shape check in `scripts/`
 * asserts that and why nothing here imports it as a value. The protocol package carries the constants and the
 * sequence tracker, and it is the one runtime dependency from the client library.
 */
export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url))
    }
  },
  // The Tauri shell serves the built assets over its own protocol, so the paths are relative to the document rather
  // than to a server root.
  base: './',
  build: {
    outDir: 'dist',
    // A source map in a release build is the whole interface in readable form, and the interface is not a secret — but
    // it doubles the bundle the user downloads, so it is off.
    sourcemap: false,
    target: 'es2022'
  }
})
