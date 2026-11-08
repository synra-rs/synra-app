import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import vue from '@vitejs/plugin-vue'
import buildStamp from 'unplugin-build-stamp/vite'
import { defineConfig } from 'vitest/config'

const coreRoot = fileURLToPath(new URL('../synra-rust-core', import.meta.url))
const buildStampModule = fileURLToPath(new URL('./src/auto-generated/build-stamp.ts', import.meta.url))
const coreVersion = /version\s*=\s*"([^"]+)"/.exec(readFileSync(new URL('../synra-rust-core/Cargo.toml', import.meta.url), 'utf8'))?.[1] ?? '0.0.0'

/**
 * The test run.
 *
 * A separate file from `vite.config.ts` because a `test` key in a Vite config is a Vitest extension Vite does not know
 * about, and one config serving both means the build's types are the union of the two.
 *
 * The environment is **Node**, not a DOM: the stores and the dispatcher are logic, and a DOM would let a test depend
 * on a component rendering — which is a different kind of test and not the one this run is for.
 */
export default defineConfig({
  plugins: [
    buildStamp({ include: [buildStampModule], log: false }),
    buildStamp({ cwd: coreRoot, include: [buildStampModule], name: '__CORE_BUILD_STAMP__', log: false, extra: { name: 'synra-core', version: coreVersion } }),
    vue()
  ],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
      // ★ `@synra/protocol` is a **type-only** package: its `package.json` has no runtime entry, so importing the
      // values it exports — `SequenceTracker`, `isDelta`, the frame kinds — fails to resolve. That is a real
      // inconsistency in that package rather than a mistake here: it exports constants a consumer needs at runtime
      // while declaring itself type-only.
      //
      // The fix here is to point at its source, which is what a workspace dependency in the same checkout can do. The
      // durable fix is for the package to gain a runtime entry for the values and keep the types type-only; until
      // then this alias is the one place that knows.
      '@synra/protocol': fileURLToPath(
        new URL('../synra-js/packages/protocol/src/index.ts', import.meta.url)
      )
    }
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts']
  }
})
