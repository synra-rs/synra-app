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
    empty: 'Nothing is moving to or from another device.',
    action: 'Manage devices',
    sending: 'Sending',
    receiving: 'Receiving',
    cancel: 'Cancel',
    confirm: 'Abandon this transfer?',
    abandon: 'Abandon',
    keep: 'Continue transfer',
    progress: '{done} of {total}'
  },

  received: {
    title: 'Received',
    empty: 'Nothing has arrived from another device yet.',
    action: 'Pair a device',
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
    empty: 'No file has landed in the inbox yet.'
  }
} as const
