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
  forgotten: (name: string): string => `Forgot ${name}.`,
  unpaired: 'Not paired',
  /** Undoes a relationship. Confirmed inline because the next send to that device is a first contact again. */
  forget: 'Forget',
  forgetTitle: (name: string): string => `Forget ${name}?`,
  forgetHelp: 'This device will be treated as new the next time it is reached. Automatic acceptance for this sender will also be removed.',
  forgetCancel: 'Keep',
  online: 'Reachable',
  offline: 'Not reachable'
} as const
