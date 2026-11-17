/**
 * The debug console's words.
 *
 * Kept with the other catalogs rather than inline: the bubble is visible on every page, so its labels are vocabulary,
 * not one screen's copy.
 */
export const debug = {
  title: 'Wire',
  all: 'All',
  open: 'Open the wire',
  close: 'Close',
  tools: 'Wire actions',
  search: 'Search',
  searchPlaceholder: 'Search wire traffic',
  searchPrevious: 'Previous match',
  searchNext: 'Next match',
  searchClose: 'Close search',
  searchNoResults: 'No matches',
  metadata: 'Line details',
  traceId: 'Trace ID',
  requestId: 'Request ID',
  peer: 'Peer',
  peerAll: 'All peers',
  event: 'Event',
  closeMetadata: 'Close details',
  clear: 'Clear',
  copy: 'Copy',
  copied: 'Copied',
  copyFailed: 'Copy failed',
  pause: 'Pause',
  resume: 'Resume',
  paused: 'Paused',
  newLines: (count: number) => `${count} new ${count === 1 ? 'line' : 'lines'}`,
  jumpLatest: 'Jump to latest',
  viewingHistory: 'Viewing history',
  empty: 'No traffic yet. Scan, pair, or send something.',
  sent: 'sent',
  received: 'received',
  note: 'note',
  lines: (count: number) => `${count} lines`,
  hintOpen: 'Esc closes',
  hintClosed: 'Live kernel traffic'
} as const
