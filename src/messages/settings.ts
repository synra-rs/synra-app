/**
 * The settings screen's words.
 *
 * The page's own key names are not the kernel's — its configuration is addressed by dotted paths (`ui.theme`) — so the
 * preference labels are labels rather than the key printed into the interface, which is also what keeps a
 * user-visible string out of the template (R31).
 */
export const settings = {
  title: 'Settings',
  device: 'This device',
  field: {
    name: 'Name',
    nameIsGenerated: 'generated — you can rename it',
    rename: 'Rename',
    save: 'Save',
    cancel: 'Cancel',
    platform: 'Platform',
    kernel: 'Kernel',
    deviceId: 'Device id'
  },
  preferences: 'Preferences',
  /**
   * The two keys this page reads, in the user's words.
   *
   * The page's own key names are not the kernel's — its configuration is addressed by dotted paths (`ui.theme`) —
   * so these are labels rather than the key printed into the interface, which is also what keeps a user-visible
   * string out of the template (R31).
   */
  preference: {
    theme: 'Theme',
    autoSyncPlugins: 'Update plugins automatically'
  },
  /** Why the theme row has no control, where a control would be. */
  themeFixed: 'One palette ships with the host; plugins read this value rather than choosing another.',
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
  transfers: {
    title: 'Transfers',
    cancelAsk: 'Ask before cancelling',
    cancelAskDetail: 'Confirm before a transfer in flight is abandoned.',
    notify: 'Notify when a file arrives',
    notifyDetail: 'A system notification, so a transfer that lands while the window is elsewhere is not missed.'
  },
  /** How long the durable audit trail is kept. The kernel sweeps the table when it starts. */
  audit: {
    title: 'Audit history',
    detail: 'Recorded plugin calls are kept for this many days, then removed the next time the kernel starts.',
    save: 'Save'
  },
  /** What a user sends when something is wrong, and the two optional sections that need a decision. */
  diagnostics: {
    title: 'Diagnostics',
    detail: 'A bundle of this device\u2019s environment, configuration, devices and plugins, for whoever is helping you. The audit trail and debug logs are opt-in, and the file records what it contains.',
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
    probesDetail: 'Checks the ports, the data folder, the clock and the runtime this device would need in order to start.',
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
    detail: 'The npm-compatible registries this device installs plugins from, tried in order. Prefix one with `static:` when it is a bucket serving one index.json per package. Leave the list empty to disable registry installs.',
    placeholder: 'https://registry.example.com',
    add: 'Add mirror',
    remove: 'Remove',
    save: 'Save'
  },
  /**
   * 本进程记录器的指标。
   *
   * 页面不解释数字的含义：指标名与标签是内核的词汇，翻译它们会让同一件事有两个名字。这里只把快照排成行。
   */
  metrics: {
    title: 'Performance',
    detail: 'Counters, gauges and histograms recorded by this process. The panel re-reads them every few seconds and contains no payload content.',
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
