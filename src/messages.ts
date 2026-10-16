/**
 * Every user-visible string in the interface, in one place (R31, R32).
 *
 * # Why one module and not strings in the templates
 *
 * Two reasons in one: the interface is bilingual (`index.html`'s `lang` note), and "the wording is written by whoever
 * touched the file last" is how four names for the same thing appear. So there is one file where the vocabulary can be
 * read at once — and the rule that matters is not "translate later", it is **one thing, one word** (R32).
 *
 * # The vocabulary, as decided
 *
 * | Thing | The word | Retired words |
 * | ----- | -------- | ------------- |
 * | Another machine running Synra | **device** | `peer`, "nearby", "shared to this device" |
 * | Something installed into Synra | **plugin** | `addon`, `extension` |
 * | This machine's own configuration | **settings** | `config`, `preferences` (two names for one screen) |
 *
 * A command's name and the words around it are the same word: `devices_scan_start` is the **Scan** button, and its
 * result is a scan. When the button says *Pair*, the thing that happened is *paired*, not *connected*.
 *
 * # What is deliberately not here
 *
 * The kernel's own `message` strings. They are written for whoever implements the kernel ("The kernel refused the
 * snapshot") and R34 keeps internal words out of the interface — so an error is rendered from *our* vocabulary, keyed by
 * the error's `code`, and the raw string is left for the diagnostics page.
 */

/** The kernel's error codes, as far as the interface says anything about them. */
export type ErrorCode
  = | 'capability_denied'
    | 'device_offline'
    | 'invalid_argument'
    | 'not_found'
    | 'plugin_faulted'
    | 'unauthorized'
    | string

export const messages = {
  app: {
    name: 'Synra'
  },

  /** The four pages, as the navigation shows them. `router.ts` reads these, so the nav cannot disagree with itself. */
  nav: {
    home: { label: 'Home', hint: 'What this device is doing' },
    devices: { label: 'Devices', hint: 'Pair and send' },
    plugins: { label: 'Plugins', hint: 'Install and grant' },
    settings: { label: 'Settings', hint: 'This device' }
  },

  home: {
    title: 'Home',

    /**
     * The wire (R27): the one thing on this screen worth spending boldness on, and the only place glass and glow are
     * allowed to mean something. Every string here describes a **real** state — there is no decorative animation, and
     * the state is written out as well as drawn, because colour alone is not a state (R23).
     */
    wire: {
      label: 'This device and the others',
      states: {
        transferring: { headline: 'Transferring', detail: 'A send is in progress.' },
        connected: { headline: 'Connected', detail: 'Ready to send.' },
        offline: { headline: 'Not reachable', detail: 'Paired, but it is not answering right now.' },
        none: { headline: 'No device yet', detail: 'Pair one to send it something.' }
      },
      action: 'Pair a device'
    },

    inFlight: {
      title: 'Sending now',
      empty: 'Nothing is on its way to another device.',
      action: 'Choose a device'
    },

    received: {
      title: 'Received',
      empty: 'Nothing has arrived from another device yet.',
      action: 'Pair a device'
    }
  },

  devices: {
    title: 'Devices',
    scan: 'Scan',
    scanning: 'Scanning',
    scanProgress: 'Found',
    scanElapsed: 'Elapsed',
    empty: 'No device has been found yet.',
    emptyAction: 'Scan for devices',
    paired: 'Paired',
    unpaired: 'Not paired',
    online: 'Reachable',
    offline: 'Not reachable'
  },

  plugins: {
    title: 'Plugins',
    empty: 'No plugin is installed.',
    emptyAction: 'See what a plugin is',
    granted: 'Granted',
    refused: 'Refused',
    none: 'nothing',
    updateAvailable: 'An update is available',
    updateNeedsConsent: 'It asks for new permission'
  },

  settings: {
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
     * Rendered next to controls that exist but do nothing yet. A toggle that looks like it works and does not is worse
     * than one that is visibly absent: the user changes it, sees nothing move, and starts debugging the wrong thing.
     */
    notWiredYet: 'These come from the kernel. Changing them is not wired up yet, so they are shown rather than offered.'
  },

  /**
   * What the interface says when the kernel refuses something (R33).
   *
   * Not an apology and not a shrug: what happened, in the interface's own words. The `code` is kept for the diagnostics
   * page — the main interface does not print raw error strings (R34).
   *
   * ★ **Two tables, and the order between them matters.** The host's `IpcError.message` is a **key**, not a sentence —
   * the interface is the side that renders it (`src/ipc/client.ts` explains why). A key is finer-grained than a code
   * (one code can have several messages), so `kernelErrors` is consulted first and `errors` is the fallback for codes
   * whose key we have not met yet. An unknown key is not blank: it falls back to the code's sentence, and then to
   * `unknown` — a chain that always says *something* true.
   */
  kernelErrors: {
    'error.badArgument': 'Something in the request was not valid.',
    'error.bridgeFailed': 'The interface could not reach the kernel.',
    'error.capabilityDenied': 'This needs a permission that has not been granted.',
    'error.commandNotFound': 'That is not something this version can do.',
    'error.storeUnavailable': 'The place that keeps this is not available.',
    'error.timeout': 'The kernel did not answer in time.'
  },

  errors: {
    capability_denied: 'This needs a permission that has not been granted.',
    device_offline: 'That device is not answering.',
    invalid_argument: 'Something in the request was not valid.',
    not_found: 'That is not here any more.',
    plugin_faulted: 'That plugin stopped working.',
    unauthorized: 'This is not allowed.',
    unknown: 'That did not work.'
  },

  /** The labels for the kernel's suggested actions, when it sends one with an error. */
  actions: {
    openSettings: 'Open settings',
    requestCapability: 'Ask for the permission',
    installPlugin: 'Open plugins',
    connectDevice: 'Open devices',
    retry: 'Try again',
    viewLogs: 'View the log'
  }
} as const
