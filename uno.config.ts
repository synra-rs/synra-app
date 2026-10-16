import { synraConfig } from '@synra/theme/unocss'

import { defineConfig } from 'unocss'

/**
 * The host interface's styles, in one line.
 *
 * # Why there is almost nothing here
 *
 * Everything the interface is allowed to say about colour, type, spacing, radius, motion and z-order lives in
 * `@synra/theme` — this file only says *which tier of preflight* to emit. `synraConfig()` also carries the
 * `blocklist`, so the utilities that would drift (`text-[#fff]`, `z-[99]`, `uppercase`, `ml-4`) are rejected by both
 * the build and `pnpm run lint` (`unocss/blocklist`, §13.1 guard 1).
 *
 * `preflight: 'app'` is the tier with the reset: this is an application, and it owns the whole document. The
 * documentation site uses `'tokens'` instead because VitePress owns its own document (R47).
 *
 * ★ This file deliberately does **not** enable `presetAttributify` or `transformerDirectives` (R5): a shortcut that
 *   exists by default is harder to bypass than a rule that has to be remembered.
 */
export default defineConfig({
  ...synraConfig({ preflight: 'app' })
})
