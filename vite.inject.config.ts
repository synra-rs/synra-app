import { defineConfig } from 'vite'

/**
 * The build of the façade a plugin page receives.
 *
 * # Why a second Vite config rather than a second entry in the app's
 *
 * The host interface is an application: it has an `index.html`, a router, Vue and a dev server. The façade is the
 * opposite of all four — one script, no document, no imports at runtime, evaluated inside somebody else's page. Putting
 * it in the app's build would make it a chunk of the app bundle, and a plugin page cannot load the app bundle (different
 * origin, no server).
 *
 * # Why IIFE and not an ES module
 *
 * The shell prepends the file to Tauri's `initialization_script`, which is a plain script: there is no `<script
 * type="module">` for an ESM build to hang from. The IIFE form also inlines the one runtime import (`@tauri-apps/api`),
 * so the page needs no network request to get its client — and the library build already disables code splitting, which
 * is what keeps it one file.
 *
 * # What reads the output
 *
 * `src-tauri/src/commands/plugin_ui.rs` `include_str!`s `src-tauri/inject/facade.js`, and `scripts/check-shapes.ts`
 * rebuilds it into a temporary directory and compares, so the committed file cannot drift from this source.
 */
export default defineConfig({
  build: {
    outDir: 'src-tauri/inject',
    // The directory is a source location for a `include_str!`, not a build output tree: emptying it on every build would
    // delete files a Rust build is about to read.
    emptyOutDir: false,
    sourcemap: false,
    target: 'es2022',
    lib: {
      entry: 'src/ipc/injection/bootstrap.ts',
      name: 'SynraInjection',
      formats: ['iife'],
      fileName: () => 'facade.js'
    }
  }
})
