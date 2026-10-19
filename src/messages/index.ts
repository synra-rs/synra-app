/**
 * Every user-visible string in the interface, in one place (R31, R32).
 *
 * # Why one module and not strings in the templates
 *
 * Two reasons in one: the interface is bilingual (`index.html`'s `lang` note), and "the wording is written by whoever
 * touched the file last" is how four names for the same thing appear. So there is one file where the vocabulary can be
 * read at once — and the rule that matters is not "translate later", it is **one thing, one word** (R32).
 *
 * # The vocabulary, as decided
 *
 * | Thing | The word | Retired words |
 * | ----- | -------- | ------------- |
 * | Another machine running Synra | **device** | `peer`, "nearby", "shared to this device" |
 * | Something installed into Synra | **plugin** | `addon`, `extension` |
 * | This machine's own configuration | **settings** | `config`, `preferences` (two names for one screen) |
 *
 * A command's name and the words around it are the same word: `devices_scan_start` is the **Scan** button, and its
 * result is a scan. When the button says *Pair*, the thing that happened is *paired*, not *connected*.
 *
 * # What is deliberately not here
 *
 * The kernel's own `message` strings. They are written for whoever implements the kernel ("The kernel refused the
 * snapshot") and R34 keeps internal words out of the interface — so an error is rendered from *our* vocabulary, keyed by
 * the error's `code`, and the raw string is left for the diagnostics page.
 *
 * # Why the object is assembled here and the pages live beside it
 *
 * The file was one long one, and the owner's rule is that a source file should not be long. The public path is
 * unchanged: `./messages` still exports `messages`, with the same keys in the same order, and `uno.config.ts` still
 * derives its `safelist` from `messages.nav`. Each screen now owns its own words in `messages/<screen>.ts`, and this
 * file is only the assembly.
 */

import { crash } from './crash'
import { devices } from './devices'
import { actions, errors, kernelErrors } from './errors'
import { home } from './home'
import { nav } from './nav'
import { plugins } from './plugins'
import { settings } from './settings'
import { transfers } from './transfers'

export type { ErrorCode } from './errors'

export const messages = {
  app: {
    name: 'Synra'
  },

  nav,

  home,

  devices,

  crash,

  plugins,

  settings,

  transfers,

  kernelErrors,

  errors,

  actions
} as const
