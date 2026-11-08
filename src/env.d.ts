/// <reference types="unplugin-build-stamp/client" />

/**
 * Ambient declarations for things the bundler resolves that TypeScript cannot.
 *
 * `uno.css` is a **virtual** module: `@unocss/vite` answers it at build time, so there is no file on disk for TypeScript
 * to resolve. Declaring it here is what lets `main.ts` import the stylesheet explicitly instead of depending on the
 * plugin to inject it (see that import for why it is explicit).
 */
declare module 'uno.css'

/** Local stylesheets apply styles without exporting a JavaScript API. */
declare module '*.css' {}

/**
 * xterm ships its stylesheet as a real file, but its package exports do not advertise the `.css` path to TypeScript.
 * The import is a side effect only — the terminal still works without the styles, it is simply unpositioned — so the
 * declaration is the whole contract.
 */
declare module '@xterm/xterm/css/xterm.css'

/**
 * The two Vite flags this project reads.
 *
 * `DEV` gates the wire console's design-preview lines. Declared narrowly rather than pulling `vite/client`, which would
 * add a second copy of Vite's ambient types beside the ones `@vue/tsconfig` already supplies.
 */
interface ImportMetaEnv {
  readonly DEV: boolean
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
