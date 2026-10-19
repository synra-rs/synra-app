/**
 * The home screen's words.
 *
 * The wire (R27): the one thing on this screen worth spending boldness on, and the only place glass and glow are
 * allowed to mean something.
 */
export const home = {
  title: 'Home',

  share: {
    placeholder: 'Paste a link or some text to share',
    action: 'Share',
    clipboard: 'Share clipboard'
  },

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
    title: 'In flight',
    empty: 'Nothing is moving to or from another device.',
    action: 'Choose a device',
    sending: 'Sending',
    receiving: 'Receiving',
    cancel: 'Cancel',
    confirm: 'Abandon this transfer?',
    abandon: 'Abandon',
    keep: 'Keep sending',
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
    trace: 'Trace'
  },

  /** The files in the inbox — the history an in-flight row cannot be. */
  receivedFiles: {
    title: 'Received files',
    empty: 'No file has landed in the inbox yet.',
    open: 'Open',
    showInFolder: 'Show in folder',
    delete: 'Delete',
    deleteConfirm: 'Delete this file from the inbox?',
    keep: 'Keep'
  }
} as const
