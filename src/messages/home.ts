/**
 * The home screen's words.
 *
 * The wire (R27): the one thing on this screen worth spending boldness on, and the only place glass and glow are
 * allowed to mean something.
 */
export const home = {
  title: 'Home',

  /**
   * The wire (R27): the one thing on this screen worth spending boldness on, and the only place glass and glow are
   * allowed to mean something. Every string here describes a **real** state — there is no decorative animation, and
   * the state is written out as well as drawn, because colour alone is not a state (R23).
   */
  wire: {
    label: 'This device and the others',
    states: {
      transferring: { headline: 'Transferring', detail: 'A transfer is in progress.' },
      connected: { headline: 'Connected', detail: 'The device is available.' },
      offline: { headline: 'Not reachable', detail: 'Paired, but it is not answering right now.' },
      none: { headline: 'No device yet', detail: 'Pair a device to connect.' }
    },
    action: 'Pair a device'
  },

  inFlight: {
    title: 'In flight',
    empty: 'No transfers in progress.',
    action: 'Manage devices',
    sending: 'Sending',
    receiving: 'Receiving',
    cancel: 'Cancel',
    confirm: 'Abandon this transfer?',
    abandon: 'Abandon',
    keep: 'Continue transfer',
    progress: '{done} of {total}',
    /**
     * ★ The row used to print `transfer.state` raw, so a Chinese screen said "active" and "offered". Those are wire names,
     * not words: `active` is a transfer with bytes moving, `offered` is a question the receiver has not answered yet.
     * No `satisfies Record<string, string>` here: it would widen the keys to `string` and the caller's `keyof` lookup
     * would stop being checked against these two names.
     */
    states: {
      active: 'In progress',
      offered: 'Waiting for the other device'
    }
  },

  received: {
    title: 'Tasks',
    empty: 'No tasks to display.',
    accept: 'Accept',
    decline: 'Decline',
    offline: 'offline',
    /** The board's camelCase states, as words rather than as the engine spells them. */
    states: {
      matching: 'Finding a handler',
      choosing: 'Choose a device',
      confirming: 'Confirm',
      dispatching: 'Running',
      succeeded: 'Done',
      failed: 'Failed',
      noHandler: 'No handler',
      timedOut: 'Timed out',
      refused: 'Refused',
      cancelled: 'Cancelled'
    },
    /** The id a person pastes into `synra logs --trace` to follow one action across devices. */
    trace: 'Trace',
    /** The disclosure that holds what the row cannot show without crowding it. */
    details: 'Details',
    startedAt: 'Started',
    /** The button copies the id alone — a label pasted into a command line would break it. */
    copy: 'Copy',
    copied: 'Copied',
    /** The clipboard refused; the id is selected instead, so the next keystroke can still copy it. */
    selectInstead: 'Selected — press Ctrl+C',
    /** A dispatch nobody has answered is being sent again; the numbers are this process's effort, not the action's. */
    retrying: 'Retrying ({attempts}/{max})',
    /** The same, with the clock time of the next attempt — an absolute time, so it stays true between events. */
    retryingAt: 'Retrying ({attempts}/{max}), next try at {time}',
    /** The retry bound was reached; the intent is still dispatching and the next start tries again. */
    noAnswer: 'No answer yet. It will try again the next time the app starts.'
  },

  /** The files in the inbox — the history an in-flight row cannot be. */
  receivedFiles: {
    title: 'Received files',
    empty: 'No file has landed in the inbox yet.',
    /**
     * `receivedAtMs` is the last write, not the start — [05 §5](../../design-docs/05-home-and-activity.md) forbids
     * labelling one as the other.
     */
    receivedAt: 'Received'
  }
} as const
