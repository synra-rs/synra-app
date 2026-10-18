/**
 * What the host says about itself.
 *
 * Every method here is a fact the host already knows, which is why they are synchronous: a plugin branching during a
 * render must not have to await the answer, and a round trip to learn something the shell was already holding is a
 * round trip that can fail.
 */

import type { ClientFeature, EnvNamespace, PlatformCapability } from '@synra/client'

import type { InjectionContext } from './context'
import { BACKED } from './capabilities'

/** The environment namespace, reading the context it was built with. */
export function createEnvNamespace(context: InjectionContext, granted: ReadonlySet<string>): EnvNamespace {
  const supported = new Set(context.platformCapabilities)

  return {
    // ★ These three read the context rather than naming a value. They used to be the literals `'desktop'`, `'windows'`
    // and `'0.1.0'`, which is a claim about the machine the plugin happens to be running on — and `'windows'` was wrong
    // on every other platform.
    hostKind: () => context.hostKind,
    platform: () => context.platform,
    locale: () => context.locale,
    darkMode: () => context.darkMode,
    appVersion: () => context.appVersion,
    sdkVersion: () => context.sdkVersion,
    // ★ Platform, not grant. A capability can be granted on a platform that cannot do it, and the interface has to
    // say a different thing in each case — so this answers only about the platform and `can` only about the grant.
    platformSupports: (capability: PlatformCapability) => supported.has(capability),
    // ★ A snapshot, read from the one table. Synchronous because a render has to be able to branch on it.
    hostSupports: (feature: ClientFeature) => BACKED[feature] === true,
    can: (capability: string) => granted.has(capability)
  }
}
