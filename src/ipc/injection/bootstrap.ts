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
 * The shell reads the built file (`src-tauri/inject/facade.js`, produced by `pnpm run build:facade`) and prepends it to
 * the window's initialization script, after a JSON object named `__SYNRA_PLUGIN__`. Tauri evaluates initialization
 * scripts **before any of the page's own scripts**, so `window.synra` exists by the time the plugin's first line runs —
 * which is the promise `@synra/client`'s documentation makes.
 *
 * # Why the seed is frozen
 *
 * The page runs somebody else's code. `Object.freeze` is applied by the shell before this file runs, and this file reads
 * the values **once** while building the context; nothing re-reads the global later. A mutable seed would let a script
 * rewrite the capabilities its own page holds and then call `env.can` against the rewritten set.
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
}

declare global {
  interface Window {
    /** Written by the shell's initialization script before this file runs. */
    readonly __SYNRA_PLUGIN__?: InjectionSeed
  }
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
    appVersion: seed.appVersion
  }
  const client = createInjection(context)
  // ★ The global is declared `readonly` in `@synra/client` (the host does not expect anyone to replace it), so the
  // assignment goes through a shape that says what is actually happening here: this module is the one writer.
  ;(window as unknown as { synra?: SynraClient }).synra = client
  return client
}

// ★ The page is a plugin page, or it is not. When the seed is absent this file does nothing rather than inventing a
// façade: a client with empty capabilities would answer every `env.can` false and every call with a refusal from a host
// that was never asked.
const seed = window.__SYNRA_PLUGIN__
if (seed !== undefined) {
  inject(seed, new TauriKernel())
}
