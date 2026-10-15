/**
 * The real bridge: the two calls the interface makes into the Tauri shell.
 *
 * # What this replaces
 *
 * `kernel.ts` had been building the client over [`FakeKernel`](./fake-kernel.ts) with a comment saying the real one "is
 * not written yet". This is it — and until it existed, the interface could only ever talk to a fixture.
 *
 * # Why `@tauri-apps/api` rather than the injected globals
 *
 * Tauri can expose its whole JavaScript API on `window.__TAURI__` (`app.withGlobalTauri`), and using that would need no
 * dependency at all. It is not what this does, because the globals are injected into **the page**, and this page is the
 * host interface. Today no third-party plugin UI is rendered in this WebView — there is no host for one yet — but the
 * design says there will be, and a decision that is safe only while a feature is missing is not a decision.
 *
 * `@tauri-apps/api` reaches the same IPC through `window.__TAURI_INTERNALS__`, which Tauri injects whether or not the
 * ergonomic globals are enabled. Importing it means the only way from this page to the kernel is through the two
 * functions below, and a future plugin UI cannot call an arbitrary command by finding an object on `window`.
 *
 * # Why the detection is on the internals and not on a flag
 *
 * Because a build-time flag is a thing that is *set*, and the failure mode of getting it wrong is a packaged
 * application that quietly runs against the fake kernel: the window opens, the pages render, and every number in them is
 * invented.
 */

import type { Bridge } from './client'

/**
 * Whether a Tauri shell is hosting this page.
 *
 * Takes the window rather than reading the global, so the decision can be tested on all of its answers — a shell, a
 * browser, and a page where the property exists but is not an object (which anything running on the page could
 * produce, and which must not be mistaken for a shell).
 */
export function hasTauriInternals(candidate: unknown = globalThis): boolean {
  if (typeof candidate !== 'object' || candidate === null) {
    return false
  }
  const internals = (candidate as Record<string, unknown>).__TAURI_INTERNALS__
  return typeof internals === 'object' && internals !== null
}

/**
 * The part of Tauri's JavaScript API this bridge uses, as a value.
 *
 * # Why the bridge takes this rather than importing it
 *
 * Two reasons, and the second is the one that matters:
 *
 * 1. `@tauri-apps/api` reads `window.__TAURI_INTERNALS__` when the **module** loads, so importing it eagerly in a
 *    browser — the dev server, a component test — either throws or leaves the module holding `undefined`. The second is
 *    worse than the first: the failure moves from "this module cannot load here" to "the first call did nothing".
 * 2. It is the only way to test what this class *adds*. Everything below is delegation except one line — unwrapping the
 *    event — and that line is exactly where the two bridges could stop being interchangeable. A test that cannot
 *    substitute the API cannot reach it, and would end up asserting on a copy of the rule instead of the rule.
 */
export interface TauriApi {
  /** Tauri's `invoke`. */
  invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
  /** Tauri's `listen`, whose handler receives an envelope rather than the payload. */
  listen: (
    event: string,
    handler: (delivered: { readonly payload: unknown }) => void
  ) => Promise<() => void>
}

/** The real API, loaded on first use. */
async function realApi(): Promise<TauriApi> {
  const [{ invoke }, { listen }] = await Promise.all([
    import('@tauri-apps/api/core'),
    import('@tauri-apps/api/event')
  ])
  return {
    invoke: (command, args) => invoke(command, args),
    listen: (event, handler) => listen(event, handler)
  }
}

/**
 * The bridge over Tauri's IPC.
 */
export class TauriKernel implements Bridge {
  /// The loaded API, kept so the dynamic import happens once.
  ///
  /// Memoised as the **promise** rather than the value, so two calls arriving before the first import resolves share one
  /// load instead of starting a second. A rejected promise is kept too: this bridge is only constructed when a shell is
  /// present, so a load that failed is a fact about the process rather than something a retry would fix, and re-importing
  /// per call would turn one clear failure into a per-command one.
  private loaded: Promise<TauriApi> | undefined

  constructor(private readonly load: () => Promise<TauriApi> = realApi) {}

  private api(): Promise<TauriApi> {
    this.loaded ??= this.load()
    return this.loaded
  }

  /** Calls a kernel command. */
  async invoke(command: string, args?: Readonly<Record<string, unknown>>): Promise<unknown> {
    const api = await this.api()
    // Passing `undefined` rather than `{}` matters: a command with no parameters and a command given an empty object are
    // different calls on Tauri's side.
    return args === undefined ? api.invoke(command) : api.invoke(command, { ...args })
  }

  /**
   * Subscribes to kernel events.
   *
   * ★ The payload is unwrapped here rather than at each subscriber. Tauri hands the handler an envelope with the value
   * under `.payload`, while a `Bridge` speaks in payloads — `FakeKernel` already delivers bare payloads, so unwrapping
   * here is what keeps the two bridges interchangeable. Forwarding the envelope would work in a shell and break in every
   * test, which is the direction of failure that survives longest.
   */
  async listen(event: string, handler: (payload: unknown) => void): Promise<() => void> {
    const api = await this.api()
    return api.listen(event, (delivered) => {
      handler(delivered.payload)
    })
  }
}
