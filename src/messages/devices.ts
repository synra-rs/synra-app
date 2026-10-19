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
  paired: 'Paired',
  unpaired: 'Not paired',
  pair: 'Pair',
  pairingTitle: 'Pairing request',
  pairingCode: 'Compare this code on both screens',
  pairingAccept: 'Accept',
  pairingDecline: 'Decline',
  /** Undoes a pairing. Confirmed inline because the next send to that device will have to pair again. */
  forget: 'Forget',
  forgetTitle: 'Forget this device? It will have to be paired again.',
  forgetCancel: 'Keep',
  online: 'Reachable',
  offline: 'Not reachable'
} as const
