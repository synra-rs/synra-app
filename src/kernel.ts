/**
 * Where the interface gets its client from.
 *
 * # Why this file exists
 *
 * `KernelClient` takes a `Bridge`, and there is exactly one implementation that will ever be real: the Tauri one, which
 * needs a WebView and therefore `src-tauri`. The decision between that and [`FakeKernel`](./ipc/fake-kernel.ts) is made
 * **here and nowhere else** on purpose. A component that reached for the fake directly would keep working after the real
 * bridge exists, and would keep working in the one place it must not: a test.
 *
 * # How the real one is chosen
 *
 * By the presence of the Tauri internals on `window`, not by an environment variable. An env var is set once per process
 * and forgotten, so the browser build and the packaged app would both keep whatever it said at startup — and a packaged
 * app silently running against a fake kernel is the failure mode this file is sized against: the window opens, the pages
 * render, and every number in them is invented.
 *
 * ★ This was a promise for several rounds: the comment above said the Tauri bridge "is not written yet", and the file
 * built the client over the fake unconditionally. [`TauriKernel`](./ipc/tauri.ts) is that bridge, so the choice is now
 * real rather than aspirational.
 */

import { KernelClient } from './ipc/client'
import { FakeKernel } from './ipc/fake-kernel'
import { hasTauriInternals, TauriKernel } from './ipc/tauri'

/**
 * The bridge this process can actually use.
 *
 * Takes the candidate rather than reading the global, so a test can ask for both answers without touching `window`.
 */
export function bridgeFor(candidate: unknown = globalThis) {
  return hasTauriInternals(candidate) ? new TauriKernel() : new FakeKernel()
}

/** The one client the whole interface shares. */
export const kernel = new KernelClient(bridgeFor())
