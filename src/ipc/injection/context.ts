/**
 * What the host needs to know about the plugin it is injecting into.
 *
 * This is the one group of facts that is not a namespace: it is the argument the facade is built from, and every
 * namespace reads a part of it. It lives apart from the assembly so that the namespaces can be read one at a time
 * without carrying the whole context through every file.
 */

import type { EnvNamespace } from '@synra/client'
import type { Platform, PluginId } from '@synra/protocol'

import type { Bridge } from '../client'

/**
 * The host shapes a plugin can be told about.
 *
 * ★ Derived rather than restated. `EnvNamespace.hostKind()` inlines `'desktop' | 'mobile'` and there is no named type
 * for it anywhere, so writing the union out again here would create a second copy that could drift from the contract
 * the plugin is compiled against. `ReturnType` reads the one that exists.
 */
export type HostKind = ReturnType<EnvNamespace['hostKind']>

/** What the host needs to know about the plugin it is injecting into. */
export interface InjectionContext {
  /** The bridge to the kernel. */
  readonly bridge: Bridge
  /** This plugin's id, for scoping storage and self-invoke when they arrive. */
  readonly pluginId: PluginId
  /**
   * The WebView this page is in, as the host labelled it.
   *
   * ★ The one thing a page cannot derive: `plugin_ui_ready` looks the label up in the host's `hosted_ui` table to find out
   * which* plugin it is talking to and which session it was given. Without it the call could say only "some page is
   * ready", which is not a fact a host can check.
   */
  readonly label: string
  /** The capabilities this plugin was granted. */
  readonly granted: readonly string[]
  /** The capabilities it was refused, which is the list a plugin may still ask about. */
  readonly deniedCapabilities: readonly string[]
  /** The host's own answers that do not need a round trip. */
  readonly locale: string
  /** Whether the user chose dark mode. */
  readonly darkMode: boolean
  /** The SDK version the plugin was built against. */
  readonly sdkVersion: string
  /**
   * The operating system the host is running on.
   *
   * ★ This is a field rather than a constant, and it was a constant. `platform()` returned the literal `'windows'`, so
   * on macOS, Linux, Android and iOS the injected interface told the plugin it was on Windows — a plugin that branched
   * on it took the Windows path everywhere. A wrong answer is worse than a refused one here, because a refusal is
   * visible and a plausible wrong answer is not.
   *
   * It belongs to the same group as `locale` and `darkMode`: facts the host already knows, which a plugin asks for
   * without a round trip. It was simply missing from that group.
   */
  readonly platform: Platform
  /** The host shape: a desktop install or a mobile one. */
  readonly hostKind: HostKind
  /** The host application's version, not the SDK's. */
  readonly appVersion: string
}
