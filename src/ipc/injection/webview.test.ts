import type { Platform } from '@synra/protocol'
import { describe, expect, it } from 'vitest'

import { detectWebview } from './webview'

/**
 * The engine facts a plugin can branch on.
 *
 * # Why the assertions are pairs
 *
 * Because the kind and the version are two different claims. A WebView2 on Windows with a user agent that does not say
 * `Edg/` is still WebView2 — the platform chose the engine — but its version is genuinely unknown, and inventing one
 * would make a minimum-version check wrong in the direction that looks safe.
 */
describe('detectWebview', () => {
  it('names WebView2 on Windows and reads Edge version', () => {
    const ua = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 Edg/120.0.2210.91'
    expect(detectWebview(ua, 'windows')).toEqual({ kind: 'webview2', version: '120.0.2210.91' })
  })

  it('does not invent a WebView2 version when the user agent omits it', () => {
    // The engine is decided by the platform. A page whose user agent says nothing about Edge is still running under
    // WebView2, and `null` is the honest version.
    expect(detectWebview('Mozilla/5.0 (Windows NT 10.0; Win64; x64)', 'windows')).toEqual({
      kind: 'webview2',
      version: null
    })
  })

  it('names WKWebView on macOS and reads the Safari version', () => {
    const ua = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15'
    expect(detectWebview(ua, 'macos')).toEqual({ kind: 'wkwebview', version: '17.0' })
  })

  it('dotted the iOS system version rather than leaving its underscores', () => {
    const ua = 'Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148'
    expect(detectWebview(ua, 'ios')).toEqual({ kind: 'wkwebview', version: '17.0' })
  })

  it('names the Android system WebView and reads its Chrome version', () => {
    const ua = 'Mozilla/5.0 (Linux; Android 14; Pixel 8 Build/AP1A; wv) AppleWebKit/537.36 (KHTML, like Gecko) Version/4.0 Chrome/120.0.6099.230 Mobile Safari/537.36'
    expect(detectWebview(ua, 'android')).toEqual({ kind: 'android-webview', version: '120.0.6099.230' })
  })

  it('names WebKitGTK and reports no version rather than the WebKit token', () => {
    // `AppleWebKit/605.1.15` is WebKit's version, not the GTK engine's. Reporting it as the engine version would make a
    // minimum-version comparison silently wrong.
    const ua = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15'
    expect(detectWebview(ua, 'linux')).toEqual({ kind: 'webkitgtk', version: null })
  })

  it('answers unknown for a platform this build does not know', () => {
    // `Platform` gains variants; a page must not be told it is on an engine nobody checked.
    expect(detectWebview('Mozilla/5.0', 'next-thing' as Platform)).toEqual({ kind: 'unknown', version: null })
  })
})
