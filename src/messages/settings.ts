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
  /**
   * Rendered next to controls that exist but do nothing yet. A toggle that looks like it works and does not is worse
   * than one that is visibly absent: the user changes it, sees nothing move, and starts debugging the wrong thing.
   */
  notWiredYet: 'These come from the kernel. Changing them is not wired up yet, so they are shown rather than offered.'
} as const
