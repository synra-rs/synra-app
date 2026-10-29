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
  pairingResume: 'Continue pairing',
  pairingRole: 'Pairing step',
  pairingCreate: 'Create an invitation',
  pairingAccept: 'Import an invitation',
  pairingReplyStep: 'Import a reply',
  pairingCreateHelp: 'Send this invitation to the other device through a trusted channel. Then import its reply here.',
  pairingTrustHelp: 'Only import content received directly from the intended device through a trusted channel. Network discovery cannot verify its identity.',
  pairingTarget: 'Other device',
  pairingChooseTarget: 'Select a nearby device',
  pairingNoTarget: 'Scan for the other device before importing its invitation or reply.',
  pairingInviteInput: 'Invitation from the selected device',
  pairingReplyInput: 'Reply from the selected device',
  pairingCopy: 'Copy',
  pairingCopied: 'Copied',
  pairingCopyFailed: 'Could not copy. Select the text above and copy it manually.',
  pairingRefresh: 'Check pairing status',
  pairingRestart: 'Start a new pairing',
  pairingCloseHelp: 'Closing this panel keeps pairing in progress. Use Cancel pairing to stop it.',
  pairingExport: 'Export this device invitation',
  pairingInvitation: 'Invitation to send to the intended device',
  pairingSelected: (name: string, id: string): string => `Selected device: ${name} (${id})`,
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
  online: 'Reachable',
  offline: 'Not reachable'
} as const
