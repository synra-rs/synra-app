/**
 * The script that gives a plugin page its `window.synra`.
 *
 * # Why this is a build entry of its own
 *
 * The host's interface and a plugin page are different documents with different origins. The façade is the same code in
 * both worlds, but it cannot be imported by the plugin's bundle: `@synra/client` is types-only on purpose, so the only
 * runtime copy is the one the host injects. This module is that copy's entry point.
 *
 * # How it reaches the page
 *
 * The shell reads the built file (`src-tauri/inject/facade.js`, produced by `pnpm run build:facade`) and puts it inside a
 * function it writes into the window's initialization script:
 *
 * ```js
 * (function (seed) { …the façade…; SynraInjection.boot(seed) })({ …the seed… });
 * ```
 *
 * Tauri evaluates initialization scripts **before any of the page's own scripts**, so `window.synra` exists by the time
 * the plugin's first line runs — which is the promise `@synra/client`'s documentation makes.
 *
 * # Why the seed is an argument and not a global
 *
 * The design document's rule is that the token lives in a closure, not on an object. A global `__SYNRA_PLUGIN__` would
 * be readable by any script the plugin's page includes — an analytics bundle, a dependency with a bad day — and a token
 * that can be read is a session that can be used from somewhere else. As the wrapper's parameter it exists only inside
 * the closure that builds the façade, and the façade copies it; nothing re-reads it later.
 */

import type { SynraClient } from '@synra/client'
import type { Platform } from '@synra/protocol'

import type { Bridge } from '../client'
import type { HostKind, InjectionContext } from './context'
import { TauriKernel } from '../tauri'
import { createInjection } from './index'

/**
 * The facts the shell writes into the page.
 *
 * Every field is decided by the host before the page loads, and every one is an **answer** rather than a capability:
 * the token is the session, the granted list is what the user allowed, and the environment facts are what the host
 * already knows. A page cannot change them (the object is frozen) and this file copies them into the façade's context.
 */
export interface InjectionSeed {
  /** The WebView label, for `plugin_ui_ready`. */
  readonly label: string
  /** The session token, decimal because JavaScript numbers cannot hold a `u128` exactly. */
  readonly token: string
  /** This plugin's id. */
  readonly pluginId: string
  /** The capabilities the user allowed. */
  readonly granted: readonly string[]
  /** The capabilities the user refused, which the page may still ask about. */
  readonly deniedCapabilities: readonly string[]
  /** The user's language. */
  readonly locale: string
  /** Whether the user chose dark mode. */
  readonly darkMode: boolean
  /** The SDK version the plugin was built against, from its manifest. */
  readonly sdkVersion: string
  /** The operating system. */
  readonly platform: Platform
  /** A desktop or a mobile host. */
  readonly hostKind: HostKind
  /** The host application's version. */
  readonly appVersion: string
  /**
   * What this **platform** can do, in the client's own spelling (`open_url`, `notify`, …).
   *
   * ★ Not the grants: a platform can support an action the user refused, and the page says different things in each
   * case. This is the answer `env.platformSupports` reads, decided before the page loads so a render can use it.
   */
  readonly platformCapabilities: readonly string[]
}

/**
 * Builds the façade over a bridge and publishes it on `window.synra`.
 *
 * Exported rather than only run as a side effect so a test can hand it a fake bridge: the real one loads Tauri's API,
 * which a jsdom test has no way to answer.
 */
export function inject(seed: InjectionSeed, bridge: Bridge): SynraClient {
  const context: InjectionContext = {
    bridge,
    label: seed.label,
    token: seed.token,
    pluginId: seed.pluginId,
    granted: seed.granted,
    deniedCapabilities: seed.deniedCapabilities,
    locale: seed.locale,
    darkMode: seed.darkMode,
    sdkVersion: seed.sdkVersion,
    platform: seed.platform,
    hostKind: seed.hostKind,
    appVersion: seed.appVersion,
    platformCapabilities: seed.platformCapabilities
  }
  const client = createInjection(context)
  // ★ The global is declared `readonly` in `@synra/client` (the host does not expect anyone to replace it), so the
  // assignment goes through a shape that says what is actually happening here: this module is the one writer.
  ;(window as unknown as { synra?: SynraClient }).synra = client
  return client
}

/**
 * The entry the shell's initialization script calls: the real Tauri bridge, and the seed it passed as an argument.
 *
 * The seed is a parameter of the wrapper the shell writes, not a global — see this file's header for why. By the time
 * this runs, the page still has no scripts of its own; `window.synra` is in place before the plugin's first line.
 */
export function boot(seed: InjectionSeed): SynraClient {
  return inject(seed, new TauriKernel())
}
