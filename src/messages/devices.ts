/**
 * The devices screen's words.
 *
 * A scan's labels are here rather than in the page because they are vocabulary like everything else: the Scan button
 * and the thing it produced are the same word.
 */
export const devices = {
  title: 'Devices',
  loading: 'Loading devices…',
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
  nearby: 'Nearby devices',
  details: 'Details',
  detailsFor: (name: string): string => `Show details for ${name}`,
  name: 'Name',
  deviceId: 'Device ID',
  address: 'Address',
  lastSeen: 'Last seen',
  unknownTime: 'Not available',
  blocked: 'Blocked',
  scanDetails: 'Scan details',
  scanId: 'Scan ID',
  scanFinished: 'Scan finished.',
  scanStopped: 'Scan stopped.',
  scanStopping: 'Waiting for the scan to stop.',
  scanCached: 'Discovery is unavailable. Showing known devices.',
  scanFailedKnown: 'Showing previously known devices. This scan did not complete.',
  scanEmpty: 'No nearby devices found. Check that both devices are on the same network, then scan again.',
  refresh: 'Refresh devices',
  knownOnly: 'Showing the last successfully loaded device information.',
  forgotten: (name: string): string => `Forgot ${name}. Pair again to reconnect.`,
  unpaired: 'Not paired',
  pair: 'Pair',
  pairingTitle: 'Pair devices',
  /*
   * The other half of pairing: a device that asked, and is waiting on this person.
   *
   * ★ The words say what discovery can and cannot prove. "Found on your network" is a fact this side observed; it is not
   * the device having proved who it is. The code is what closes that gap, so it is described as something to compare
   * rather than as an identifier.
   */
  pairingRequests: 'Pairing requests',
  pairingRequestWants: (name: string): string => `${name} wants to pair with this device.`,
  pairingRequestCompare: (code: string): string => `Check that the other device shows ${code}.`,
  pairingOutgoingTitle: 'Waiting for the other device',
  pairingOutgoingWants: (name: string): string => `${name} is deciding whether to pair with this device.`,
  pairingOutgoingStop: 'Hide',
  pairingRequestAddress: (address: string): string => `Reached at ${address}.`,
  pairingRequestExpires: (seconds: number): string => `Expires in ${seconds}s.`,
  pairingRequestAccept: 'Accept',
  pairingRequestDecline: 'Decline',
  pairingRequestGone: 'That request is no longer waiting. Refresh to see the current list.',
  /*
   * ★ Said plainly, because the alternative is a button that looks pressable and refuses.
   *
   * The protocol has the server side of `synra/pairing/1` — a device can receive and answer a request — but nothing on
   * this side can open one. That is a missing client, not a setting: no amount of retrying makes it work.
   */
  pairingRequestPeer: (name: string): string => `${name} is selected.`,
  pairingRequestUnavailable: 'Starting a pairing from this device is not available yet: it can answer a request from another device, but cannot send one.',
  pairingResume: 'Continue pairing',
  pairingChooseTarget: 'Select a nearby device',
  pairingRefresh: 'Check pairing status',
  pairingRestart: 'Start a new pairing',
  pairingCloseHelp: 'Closing this panel keeps pairing in progress. Use Cancel pairing to stop it.',
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
  forgetTitle: (name: string): string => `Forget ${name}?`,
  forgetHelp: 'This device will need to be paired again to reconnect. Automatic acceptance for this sender will also be removed.',
  forgetCancel: 'Keep',
  online: 'Reachable',
  offline: 'Not reachable'
} as const
