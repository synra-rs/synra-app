import { fileURLToPath } from 'node:url'

import vue from '@vitejs/plugin-vue'
import UnoCSS from 'unocss/vite'
import { defineConfig } from 'vite'

/**
 * The host interface's build.
 *
 * `@synra/client` is a **type-only** package: it has no runtime entry, which is why the shape check in `scripts/`
 * asserts that and why nothing here imports it as a value. The protocol package carries the constants and the
 * sequence tracker, and it is the one runtime dependency from the client library.
 *
 * `@synra/theme` supplies the styles through `uno.config.ts` alone — nothing imports it at runtime, so it is a
 * devDependency rather than a dependency.
 */
export default defineConfig({
  plugins: [vue(), UnoCSS()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url))
    }
  },
  // The Tauri shell serves the built assets over its own protocol, so the paths are relative to the document rather
  // than to a server root.
  base: './',
  server: {
    watch: { ignored: ['**/src-tauri/target/**'] }
  },
  build: {
    outDir: 'dist',
    // A source map in a release build is the whole interface in readable form, and the interface is not a secret — but
    // it doubles the bundle the user downloads, so it is off.
    sourcemap: false,
    target: 'es2022',
    // ★ **CSS compatibility is set here, not in UnoCSS** ([28 §4 R15](../../design-docs/28-app-ui.md)).
    //
    // `presetWind3` has no `targets` option — the earlier note that asked for one named a knob that does not exist.
    // What does exist is Vite's `cssTarget`, which esbuild uses to decide both what CSS to downlevel and what to
    // prefix. The matrix in [08 §6.1](../../design-docs/08-ipc-and-frontend.md) has WebKitGTK as the weakest engine and
    // WKWebView as the second, so Safari is named explicitly — and the concrete payoff is visible in the built CSS:
    // `backdrop-filter` (the frosted layers, [28 §6 R43](../../design-docs/28-app-ui.md)) needs the `-webkit-` form on
    // WebKit, and esbuild emits it only when a Safari target is named.
    //
    // 🟡 The numbers are a starting point, not a measurement: [28 §4 R14](../../design-docs/28-app-ui.md) leaves the
    // WebKitGTK run open (§16.2 Q-3), and the value should follow that result rather than precede it.
    cssTarget: ['chrome105', 'safari15.4']
  }
})
