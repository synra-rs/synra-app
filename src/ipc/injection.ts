/**
 * The plugin-facing module: `./ipc/injection`, unchanged for everyone who imports it.
 *
 * The surface itself is assembled in [`./injection/index.ts`](./injection/index.ts), which is now a directory so that
 * each group of the facade — the environment, the log, events, devices, the namespaces that refuse — is a file with one
 * job instead of one long file. This module stays where it was, with the same public names, so an import path that
 * existed before the split keeps resolving and keeps exporting exactly what it exported.
 */

export type { HostKind, InjectionContext } from './injection/context'
export { createInjection } from './injection/index'
