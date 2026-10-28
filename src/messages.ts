/**
 * Every user-visible string in the interface: `./messages`, unchanged for everyone who imports it.
 *
 * The object itself is assembled in [`./messages/index.ts`](./messages/index.ts), which is now a directory so that each
 * screen owns its own words in a file with one job instead of one long file. This module stays where it was, with the
 * same reactive `messages` export. English defines the schema and Chinese supplies every text key. UnoCSS reads the
 * language-independent icon names directly from `messages/nav.ts`.
 */

export type { ErrorCode } from './messages/errors'
export { messages } from './messages/index'
