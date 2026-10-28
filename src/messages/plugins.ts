/**
 * The plugins screen's words.
 *
 * One label per machine fact, so no two of them are ever joined by a separator (R21).
 */
export const plugins = {
  title: 'Plugins',
  states: {
    installed: 'Installed',
    validated: 'Validated',
    granted: 'Permissions granted',
    active: 'Active',
    deactivated: 'Disabled',
    faulted: 'Stopped after an error',
    parked: 'Stopped'
  },
  empty: 'No plugin is installed.',
  /** The filter above the installed list; it matches the id, the title and the description. */
  filterPlaceholder: 'Filter installed plugins',
  /** Why the installed list is empty when a filter kept everything out. */
  noMatch: 'No installed plugin matches that filter.',
  clearFilter: 'Clear filter',
  /** How an install names its source: a directory on this device, or a package in the configured registry. */
  installMode: {
    path: 'From a directory',
    registry: 'From the registry'
  },
  /** An update the registry publishes for an installed plugin, which the user asked to look up. */
  checkUpdates: 'Check for updates',
  lastChecked: 'Last checked {time}',
  updateFound: 'Version {version} is published',
  updateNoNewPermissions: 'It asks for the same permissions.',
  updateNewPermissions: 'It asks for more:',
  updateAction: 'Update',
  /** The result of an update, kept on the page after the toast is gone. */
  updatedTo: 'Updated {pluginId} to {version}',
  /** A faulted plugin's load can be tried again without changing its switches. */
  retry: 'Retry',
  installName: 'Plugin id, e.g. @acme/hello',
  /** The button that opens the platform's directory picker for a local install. */
  chooseDirectory: 'Choose folder',
  choosePackage: 'Choose package',
  /** What a registry package says about itself, before any of it is downloaded. */
  preview: {
    title: 'Install this plugin?',
    publisher: 'Publisher',
    directoryKey: 'The index lists this key for the publisher',
    permissions: 'It asks for',
    none: 'It asks for no permissions.',
    headless: 'Runs without an interface',
    ui: 'Adds pages to the interface',
    size: 'About {size} MiB, as published'
  },
  audit: {
    title: 'Recent calls',
    allowed: 'allowed',
    refused: 'refused',
    empty: 'No calls have been recorded.',
    // ★ Shown only beside rows that exist: clearing the empty state would be a button whose only effect is to redraw it.
    clear: 'Clear history',
    /** The next page of the durable history, offered only while the last page came back full. */
    more: 'Show more',
    /** The action's trace, which a bug report can quote for `synra logs --trace`. */
    trace: 'trace'
  },
  /** The quota a plugin is held to, as the numbers a user can read on its card. */
  limits: 'Limits',
  allLimits: 'All limits',
  limitMemory: 'memory',
  limitInstances: 'instances',
  limitCalls: 'calls/s',
  limitCallTime: 'call timeout',
  limitStorage: 'storage',
  limitTransfers: 'open transfers',
  limitBurst: 'burst',
  limitEvents: 'events/s',
  limitNetwork: 'network/s',
  limitConcurrent: 'network at once',
  limitSends: 'device sends/s',
  limitFsOps: 'filesystem ops/s',
  limitFuel: 'fuel per call',
  /** Files the system would not let go during a removal; the report carries their paths. */
  leftBehind: (count: number) => `${count} path(s) could not be removed`,
  /** The registry's own index, searched by keyword. */
  catalog: {
    placeholder: 'Search the registry',
    action: 'Search',
    empty: 'Nothing matched.',
    preview: 'Preview',
    sort: 'Order',
    byRelevance: 'Relevance',
    byName: 'Name',
    loadMore: 'Load more'
  },
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
  removeDetail: (title: string): string =>
    `Removing ${title} deletes its installed files. Its data \u2014 settings, indexes and anything it stored \u2014 stays on disk unless you choose to delete it.`,
  removeDeleteData: 'Also delete its data',
  /**
   * The quota ladder, in the words the card shows.
   *
   * `good` is not drawn — a card that announced "running normally" on every plugin would be a card whose warnings stopped
   * standing out — but the label exists so the three drawn rungs are named in one place rather than in the template.
   */
  healthLabel: (health: string): string => health === 'overreaching'
    ? 'Asking more often than its limits allow'
    : health === 'degraded'
      ? 'Running at a reduced quota'
      : health === 'faulted'
        ? 'Disabled after repeated refusals'
        : 'Running normally',
  /** The directory the checkbox would delete, so the user can see it before agreeing. */
  removeDataPath: (path: string): string => `Data folder: ${path}`,
  removedKept: (title: string, freed: string): string => `${title} was removed; ${freed} freed, and its data was kept.`,
  removedDeleted: (title: string, freed: string): string => `${title} was removed; ${freed} freed, and its data was deleted.`,
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
