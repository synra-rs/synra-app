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
    action: 'Choose a device',
    cancel: 'Cancel',
    progress: '{done} of {total}'
  },

  received: {
    title: 'Received',
    empty: 'Nothing has arrived from another device yet.',
    action: 'Pair a device'
  }
} as const
