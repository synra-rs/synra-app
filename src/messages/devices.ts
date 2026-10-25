/**
 * The devices screen's words.
 *
 * A scan's labels are here rather than in the page because they are vocabulary like everything else: the Scan button
 * and the thing it produced are the same word.
 */
export const devices = {
  title: 'Devices',
  scan: 'Scan',
  scanning: 'Scanning',
  scanProgress: 'Found',
  scanElapsed: 'Elapsed',
  /**
   * ★ A scan reports three facts, so it has three labels. Folding elapsed and total into one line ("1200 ms of
   * 3000 ms") is the join R21 retires — the separator is what an author reaches for when the parts have no names —
   * and a number with no label is one the reader has to guess at.
   */
  scanTotal: 'Total',
  empty: 'No device has been found yet.',
  emptyAction: 'Scan for devices',
  cancelScan: 'Cancel scan',
  paired: 'Paired',
  unpaired: 'Not paired',
  pair: 'Pair',
  pairingTitle: 'Pair devices',
  pairingInstructions: 'Export an invitation on the first device. On the second device select the first and import its invitation to create a reply. Return the reply to the first device and import it there. Only import content obtained directly from the intended device through a trusted channel; network discovery cannot verify its identity.',
  pairingExport: 'Export this device invitation',
  pairingInvitation: 'Invitation to send to the intended device',
  pairingSelected: (name: string, id: string): string => `Selected device: ${name} (${id})`,
  pairingImport: 'Invitation or reply from the selected device',
  pairingListen: 'Approve invitation and create reply',
  pairingConnect: 'Approve reply and connect',
  pairingReply: 'Return this reply to the first device before the deadline',
  pairingPeer: (id: string): string => `Pairing with device: ${id}`,
  pairingExpires: 'Deadline:',
  pairingCancel: 'Cancel pairing',
  pairingPhases: {
    listening: 'Waiting for the first device to import this reply',
    connecting: 'Connecting to the selected device',
    confirming: 'Confirming and saving authorization',
    cancelling: 'Cancelling pairing',
    paired: 'Paired',
    cancelled: 'Pairing cancelled',
    timedOut: 'Pairing expired; start again with a new reply',
    failed: 'Pairing failed'
  },
  /** Undoes a pairing. Confirmed inline because the next send to that device will have to pair again. */
  forget: 'Forget',
  forgetTitle: 'Forget this device? It will have to be paired again.',
  forgetCancel: 'Keep',
  /** Sending a text is the send path that needs no file dialog: the kernel turns it into a file on the other side. */
  sendText: 'Send text',
  sendFile: 'Send file',
  sendTitle: (name: string): string => `Send to ${name}`,
  sendPlaceholder: 'What do you want to send?',
  sendAction: 'Send',
  sendCancel: 'Cancel',
  /** What a completed send leaves on the screen. */
  sentMessage: 'Sent the message.',
  sentFile: (name: string): string => `Sent ${name}.`,
  online: 'Reachable',
  offline: 'Not reachable'
} as const
