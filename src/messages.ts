/**
 * Every user-visible string in the interface: `./messages`, unchanged for everyone who imports it.
 *
 * The object itself is assembled in [`./messages/index.ts`](./messages/index.ts), which is now a directory so that each
 * screen owns its own words in a file with one job instead of one long file. This module stays where it was, with the
 * same `messages` export — `uno.config.ts` reads `messages.nav` from here to build its `safelist`, and the pages and
 * `router.ts` read the rest.
 */

export type { ErrorCode } from './messages/errors'
export { messages } from './messages/index'
