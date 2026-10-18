/**
 * The plugins screen's words.
 *
 * One label per machine fact, so no two of them are ever joined by a separator (R21).
 */
export const plugins = {
  title: 'Plugins',
  empty: 'No plugin is installed.',
  /** The filter above the installed list; it matches the id, the title and the description. */
  filterPlaceholder: 'Filter installed plugins',
  noMatch: 'No installed plugin matches that filter.',
  /** How an install names its source: a directory on this device, or a package in the configured registry. */
  installMode: {
    path: 'From a directory',
    registry: 'From the registry'
  },
  installName: 'Plugin id, e.g. @acme/hello',
  /**
   * The publishers whose signatures this device has seen. The key, not the name, is what a decision is about: a publisher
   * id signed with a different key is not the publisher a user trusted.
   */
  publishers: {
    title: 'Publishers',
    detail: 'Keys that have signed a package this device verified.',
    empty: 'No signed package has been verified yet.',
    trust: 'Trust key',
    revoke: 'Revoke key'
  },
  /** The question an install of a signed package asks next. */
  publisherPrompt: {
    title: 'This package was signed',
    detail: 'Compare this fingerprint with the one the publisher gave you. Trusting the key is a decision about future packages from it.',
    trust: 'Trust this key',
    revoke: 'Never trust this publisher',
    later: 'Not now'
  },
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
  updateNeedsConsent: 'It asks for new permission',
  /** The list the user decides about, one checkbox each. */
  requested: 'It asks for',
  /** The button that records the decision. "Allow" rather than "Save": the act is approval, not persistence. */
  consentAction: 'Allow selected',

  /**
   * The install form's words, added when the command became reachable from this window.
   *
   * `installPath` is a **path**, not a package name: the shell has no registry client yet, and a field labelled
   * "package" that only accepted a directory would be a lie about what it does.
   */
  installPath: 'Directory to install from',
  installAction: 'Install',
  installing: 'Installing…',
  /**
   * Shown after a successful install, and the sentence is deliberate: the plugin is **not on** yet. Installing and
   * approving are two acts, and a user who was told "installed" and found a plugin that does nothing would go looking
   * for a fault that is not there.
   */
  installedSwitchedOff: 'Installed. It is switched off until you allow what it asks for.',
  /** Removing, and the sentence that says what happens to the user's data. */
  removeAction: 'Remove',
  removeTitle: 'Remove this plugin?',
  removeKeepsData: 'The plugin is removed and its data stays on disk. Nothing you put into it is deleted.',
  /**
   * Opening the plugin's own page.
   *
   * ★ Only drawn when the card says `hasUi`. A button on a headless-only plugin would be a control that can only
   * refuse, and the card already has the word for what the plugin is.
   */
  openAction: 'Open',
  /**
   * Sending the artifact to the paired devices that do not have it.
   *
   * The plan and the run are one button because the user's intent is one: the intermediate plan is not a decision, it is
   * the kernel telling the page what it would do.
   */
  syncAction: 'Sync to devices',
  syncing: 'Syncing…',
  /** Nothing to send: every paired device already reported this version. */
  syncNothing: 'Every paired device already has this version.',
  /** The counts as one sentence, so the three numbers cannot be read as a list of separate outcomes. */
  syncDone: (updated: number, unchanged: number, failed: number): string =>
    `Synced: ${updated} updated, ${unchanged} already current, ${failed} failed.`
} as const
