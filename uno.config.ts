import { synraAntdFloatingCss } from '@synra/theme/antd'
import { synraConfig } from '@synra/theme/unocss'

import { defineConfig } from 'unocss'

import { nav } from './src/messages/nav'

/**
 * The host interface's styles, in one place.
 *
 * # Why there is almost nothing here
 *
 * Everything the interface is allowed to say about colour, type, spacing, radius, motion and z-order lives in
 * `@synra/theme` — this file only says *which tier of preflight* to emit, and adds the one piece of CSS that comes from
 * the component library's adapter. `synraConfig()` also carries the `blocklist`, so the utilities that would drift
 * (`text-[#fff]`, `z-[99]`, `uppercase`, `ml-4`) are rejected by both the build and `pnpm run lint`
 * (`unocss/blocklist`, §13.1 guard 1).
 *
 * `preflight: 'app'` is the tier with the reset: this is an application, and it owns the whole document. The
 * documentation site uses `'tokens'` instead because VitePress owns its own document (R47).
 *
 * ★ This file deliberately does **not** enable `presetAttributify` or `transformerDirectives` (R5): a shortcut that
 *   exists by default is harder to bypass than a rule that has to be remembered.
 *
 * ★ **`synraAntdFloatingCss()` is a preflight, not a `<style>` block and not a runtime injection.** It is the one part
 *   of the component library's appearance that tokens cannot express (`backdrop-filter`, R52/R53), and the stylesheet
 *   UnoCSS already emits is where "our CSS" can be found. It also keeps `main.ts` as mount code.
 */
export default defineConfig({
  ...synraConfig({ preflight: 'app' }),

  /**
   * ★ **The navigation's icons, named in data rather than in markup.**
   *
   * UnoCSS extracts class names from source *text*, so a name that only ever appears as a value (`messages.nav.home`'s
   * `icon`) is invisible to it — which is exactly how `i-carbon-home` and `i-carbon-settings` came to be missing while
   * the other two icons rendered. Deriving the list from the same object the sidebar renders from means the two can
   * never disagree, and adding a page stays one edit.
   */
  safelist: Object.values(nav).map(page => page.icon),

  preflights: [
    {
      getCSS: () => synraAntdFloatingCss()
    }
  ]
})
