/**
 * Ambient declarations for things the bundler resolves that TypeScript cannot.
 *
 * `uno.css` is a **virtual** module: `@unocss/vite` answers it at build time, so there is no file on disk for TypeScript
 * to resolve. Declaring it here is what lets `main.ts` import the stylesheet explicitly instead of depending on the
 * plugin to inject it (see that import for why it is explicit).
 */
declare module 'uno.css'
