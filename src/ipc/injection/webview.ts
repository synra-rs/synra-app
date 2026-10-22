/**
 * Which WebView engine is rendering a page, and which version.
 *
 * # Why the platform decides the engine
 *
 * Because the host chooses the engine by platform, not the page: Windows runs WebView2, macOS and iOS run WKWebView, Linux
 * runs WebKitGTK, Android runs the system WebView. A user agent that claims otherwise (an embedded browser, a spoofed
 * string) is describing a browser that is not the one hosting this page. The user agent is read for the **version** only,
 * where the engine actually puts one.
 *
 * # Why this is progressive enhancement and never a gate
 *
 * Because the design's matrix says a page must work on the weakest engine it supports. A plugin branching on this to
 * decide whether a feature exists would be a plugin that breaks on the next engine; branching on it to skip an animation
 * is what it is for.
 */

import type { WebviewInfo } from '@synra/client'
import type { Platform } from '@synra/protocol'

/** The first capture group of `pattern` in `text`, or `null`. */
function capture(text: string, pattern: RegExp): string | null {
  const found = pattern.exec(text)
  return found?.[1] ?? null
}

/**
 * The engine and its version.
 *
 * # What a `null` version means
 *
 * That the engine does not put one in the user agent, not that the engine is unknown. WebKitGTK reports an
 * `AppleWebKit/605.1.15` token, which is WebKit's version rather than the GTK engine's — reporting it as "the version"
 * would make a minimum-version check silently wrong.
 */
export function detectWebview(userAgent: string, platform: Platform): WebviewInfo {
  switch (platform) {
    case 'windows':
      return { kind: 'webview2', version: capture(userAgent, /Edg\/(\d+(?:\.\d+)*)/) }
    case 'macos':
      return { kind: 'wkwebview', version: capture(userAgent, /Version\/(\d+(?:\.\d+)*)/) }
    case 'ios': {
      // iOS writes its version with underscores (`OS 17_0`), and a dotted version is what a comparison expects.
      const version = capture(userAgent, /OS (\d+(?:_\d+)*)/)?.replaceAll('_', '.') ?? null
      return { kind: 'wkwebview', version }
    }
    case 'linux':
      return { kind: 'webkitgtk', version: null }
    case 'android':
      return { kind: 'android-webview', version: capture(userAgent, /Chrome\/(\d+(?:\.\d+)*)/) }
    default:
      // A platform this build does not know — a future OS, or a test. `unknown` is the only honest answer, and a plugin
      // that treats it as "modern" is one the design already warns about.
      return { kind: 'unknown', version: null }
  }
}
