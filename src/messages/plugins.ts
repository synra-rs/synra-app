/**
 * The plugins screen's words.
 *
 * One label per machine fact, so no two of them are ever joined by a separator (R21).
 */
export const plugins = {
  title: 'Plugins',
  empty: 'No plugin is installed.',
  /**
   * ★ This said "See what a plugin is". There is no screen with that answer — the explanation lives in the
   * documentation site, and this window cannot open it (`default-src 'self'`, and the plugin facade's
   * `platform:open-url` is not part of the host's surface) — so that label would have been a button that does
   * nothing, which is the one failure this interface is sized against. What this page *can* do is read the list
   * again, so that is what its empty state offers (R28).
   */
  emptyAction: 'Check again',
  /** One label per machine fact, so no two of them are ever joined by a separator (R21). */
  pluginId: 'Plugin id',
  version: 'Version',
  publisher: 'Publisher',
  granted: 'Granted',
  refused: 'Refused',
  none: 'nothing',
  updateAvailable: 'An update is available',
  updateNeedsConsent: 'It asks for new permission'
} as const
