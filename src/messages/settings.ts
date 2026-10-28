/**
 * The settings screen's words.
 *
 * The page's own key names are not the kernel's — its configuration is addressed by dotted paths (`ui.theme`) — so the
 * preference labels are labels rather than the key printed into the interface, which is also what keeps a
 * user-visible string out of the template (R31).
 */
export const settings = {
  credentials: {
    title: 'Device identity',
    detail: 'Device keys and pairing status. Replacing a key requires pairing again.',
    refresh: 'Refresh identity',
    fingerprint: 'Public certificate fingerprint',
    backend: 'Key storage',
    enrollment: 'Saved identity',
    recorded: 'Saved',
    incomplete: 'Missing or incomplete',
    registrationLost: 'The saved registration is missing. The last registered fingerprint is shown for recovery. Confirm repair to replace the key and revoke all previous pairings.',
    runtime: 'Current identity',
    loaded: 'Using saved identity',
    unavailable: 'Unavailable',
    retired: 'Old keys awaiting cleanup',
    absent: 'None',
    repair: 'Repair device identity',
    cleanup: 'Retry old key cleanup',
    restart: 'Restart the application after successful repair to load the new identity, then pair devices again. If its key is missing, repair it first.',
    repaired: 'New identity saved. Previous pairings have been revoked.',
    remaining: (count: number): string => `${count} old key slots remain. Their original storage backend is required for cleanup.`,
    confirmTitle: 'Replace this device key?',
    confirmDetail: 'This revokes all existing pairings and cancels pending pairing attempts. The application must restart afterwards. Confirm the fingerprint shown below; a changed enrollment will be refused.',
    confirm: 'Revoke pairings and replace key'
  },
  title: 'Settings',
  device: 'This device',
  field: {
    name: 'Name',
    nameIsGenerated: 'generated — you can rename it',
    rename: 'Rename',
    save: 'Save',
    cancel: 'Cancel',
    platform: 'Platform',
    kernel: 'Version',
    deviceId: 'Device id'
  },
  preferences: 'Preferences',
  advanced: 'Advanced settings',
  /**
   * The one key this page reads, in the user's words.
   *
   * The page's own key names are not the kernel's — its configuration is addressed by dotted paths (`ui.theme`) —
   * so these are labels rather than the key printed into the interface, which is also what keeps a user-visible
   * string out of the template (R31).
   */
  preference: {
    theme: 'Theme'
  },
  /** Why the theme row has no control, where a control would be. */
  themeFixed: 'Dark appearance',
  /**
   * The devices this one accepts files from without asking. The list is the consequence of a choice made at the prompt,
   * and removing a row here is how that choice is taken back.
   */
  trusted: {
    title: 'Files without asking',
    detail: 'These devices can send files without a prompt.',
    empty: 'No device is trusted to send without asking.',
    remove: 'Ask again'
  },
  /** What a transfer asks before it is abandoned; the friction is a preference, not a fixed rule. */
  /** Whether Synra starts with the user's session. The platform owns the registration; the switch only asks. */
  startup: {
    title: 'Startup',
    launchAtLogin: 'Launch at login',
    launchAtLoginDetail: 'Keep Synra available after you sign in.',
    unavailable: 'This build cannot register itself to start at login.'
  },
  transfers: {
    title: 'Transfers',
    cancelAsk: 'Ask before cancelling',
    cancelAskDetail: 'Confirm before stopping a transfer.',
    notify: 'Notify when a file arrives',
    notifyDetail: 'Show a system notification for received files.'
  },
  /** How long the durable audit trail is kept. The kernel sweeps the table when it starts. */
  audit: {
    title: 'Audit history',
    detail: 'Days to keep plugin activity records.',
    save: 'Save'
  },
  /** What a user sends when something is wrong, and the two optional sections that need a decision. */
  diagnostics: {
    title: 'Diagnostics',
    detail: 'Export a troubleshooting report. Extra logs are optional.',
    audit: 'Include the audit trail',
    auditDetail: 'It names the plugins you run.',
    debug: 'Include debug log lines',
    debugDetail: 'They are the most likely to contain something you did not mean to share.',
    lines: 'Log lines',
    export: 'Export diagnostics',
    included: (sections: string): string => `Includes: ${sections}`,
    excluded: (sections: string): string => `Left out: ${sections}`,
    /** The startup probes: what this device would need in order to start. */
    probes: 'Run checks',
    probesDetail: 'Check the local environment.',
    probeStatus: (status: string): string => status === 'ok' ? 'OK' : status === 'warn' ? 'Warning' : 'Failed',
    /** Counts by status, so a report of twelve results is one line before it is twelve. */
    probesSummary: (ok: number, warn: number, fail: number): string =>
      `${ok} OK \u00B7 ${warn} warning${warn === 1 ? '' : 's'} \u00B7 ${fail} failed`,
    onlyProblems: 'Only problems',
    /** The shell's own recent log, for a user who wants to see it before exporting. */
    logs: 'Recent log',
    logsEmpty: 'The log is empty.',
    /** The crash prompt's "ignore": safe mode is decided from the reports on disk. */
    dismissCrashes: 'Dismiss crash reports',
    showCrashes: 'Show crash reports',
    crashesEmpty: 'No crash report is on disk.',
    crashNoMessage: 'the panic carried no message',
    dismissDetail: 'A repeated crash is why the next start may begin without plugins. Dismissing the reports makes it start normally again; export a bundle first if you want to keep them.',
    dismissed: 'Dismissed. The next start will load plugins again.'
  },
  /** Where registry installs are fetched from; the list is tried in order, and empty means the feature is off. */
  registry: {
    title: 'Registry',
    detail: 'Plugin sources, checked in order. Use static: for static catalogs; leave empty to disable registry installs.',
    placeholder: 'https://registry.example.com',
    add: 'Add mirror',
    remove: 'Remove',
    save: 'Save'
  },
  /** Daily checks and the user's policy for verified updates from trusted publishers. */
  updates: {
    title: 'Plugin updates',
    detail: 'Checked daily. Automatic updates require the same publisher and no new permissions.',
    policy: 'Update policy',
    never: 'Check manually',
    notifyOnly: 'Notify only',
    autoMinor: 'Automatically update within the same major version',
    autoAll: 'Automatically update all stable versions'
  },
  /**
   * 本进程记录器的指标。
   *
   * 页面不解释数字的含义：指标名与标签是内核的词汇，翻译它们会让同一件事有两个名字。这里只把快照排成行。
   */
  metrics: {
    title: 'Performance',
    detail: 'Local runtime metrics. Refreshes while advanced settings are open.',
    refresh: 'Refresh',
    empty: 'No metric has been recorded yet.',
    /** The M4 cold-start budget, read from the one series that measures the whole shell start. */
    startup: 'Startup',
    startupWithin: 'within the 500 ms budget',
    startupOver: 'over the 500 ms budget',
    /** 一个直方图的两个可读数字：多少次观测、平均多少。 */
    histogram: (count: number, mean: string): string => `${count} samples, mean ${mean}`,
    /** 有观测被折进 overflow 序列时，快照不再是全集 —— 说出来，而不是画一条看起来完整的列表。 */
    overflow: (count: number, cap: number): string =>
      `${count} observations were folded into an overflow series; the cap is ${cap} per metric.`
  }
} as const
