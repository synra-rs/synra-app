/**
 * The debug console's words.
 *
 * Kept with the other catalogs rather than inline: the bubble is visible on every page, so its labels are vocabulary,
 * not one screen's copy.
 */
export const debug = {
  title: 'Wire',
  all: 'All',
  subtitle: 'Every frame, both directions',
  open: 'Open the wire',
  close: 'Close',
  clear: 'Clear',
  copy: 'Copy',
  copied: 'Copied',
  copyFailed: 'Copy failed',
  pause: 'Pause',
  resume: 'Resume',
  empty: 'No traffic yet. Scan, pair, or send something.',
  sent: 'sent',
  received: 'received',
  note: 'note',
  lines: (count: number) => `${count} lines`,
  hintOpen: 'Esc closes',
  hintClosed: 'Live kernel traffic'
} as const