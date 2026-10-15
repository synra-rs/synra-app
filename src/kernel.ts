/**
 * Where the interface gets its client from.
 *
 * # Why this file exists
 *
 * `KernelClient` takes a `Bridge`, and there is exactly one implementation that will ever be real: the Tauri one, which
 * needs a WebView and therefore `src-tauri`. That is not written yet, so the client is built over
 * [`FakeKernel`](./ipc/fake-kernel.ts).
 *
 * The decision is made **here and nowhere else** on purpose. A component that reached for the fake directly would keep
 * working after the real bridge exists, and would keep working in the one place it must not: a test. Swapping this file
 * is the whole of the change from fake to real.
 *
 * # How the real one will be chosen
 *
 * By the presence of the Tauri internals on `window`, not by an environment variable. An env var is set once per
 * process and forgotten, so the browser build and the packaged app would both keep whatever it said at startup — and a
 * packaged app silently running against a fake kernel is the failure mode this file is sized against.
 */

import { KernelClient } from './ipc/client'
import { FakeKernel } from './ipc/fake-kernel'

/** The one client the whole interface shares. */
export const kernel = new KernelClient(new FakeKernel())
