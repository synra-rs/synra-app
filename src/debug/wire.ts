/**
 * How one wire line is written into the terminal.
 *
 * Kept out of the component so the ANSI contract is testable without a canvas: xterm interprets these strings, and a
 * colour code that a test can read is the only way to know a line is green because it was *received* rather than because
 * it happened to look green.
 */
export interface WireLine {
  atMs: number
  direction: string
  peer: string
  label: string
  bytes: number
}

const RESET = '\x1b[0m'
const DIM = '\x1b[90m'
const WHITE = '\x1b[97m'
const SENT = '\x1b[38;5;45m'
const RECEIVED = '\x1b[38;5;120m'
const NOTE = '\x1b[38;5;220m'
const NOTE_TEXT = '\x1b[38;5;223m'
const PEER = '\x1b[38;5;177m'

/** `HH:MM:SS.mmm`, in the reader's own timezone. */
export function formatWireTime(atMs: number): string {
  const date = new Date(atMs)
  const pad = (value: number, width = 2) => value.toString().padStart(width, '0')
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`
}

/** A size a human reads at a glance; a byte count with five digits is noise in a narrow drawer. */
export function formatWireBytes(bytes: number): string {
  if (bytes < 1024)
    return `${bytes}B`
  if (bytes < 1024 * 1024)
    return `${(bytes / 1024).toFixed(1)}KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`
}

/** Short enough that the event name stays the widest part of the line. */
export function shortPeer(peer: string): string {
  if (peer === 'local')
    return 'local'
  return peer.length > 8 ? `${peer.slice(0, 8)}…` : peer
}

/**
 * One line of ANSI.
 *
 * Direction is the first glyph, not a word: a terminal reader scans the left edge, and two arrows are what make a
 * conversation's shape visible without reading every label.
 */
export function formatWireLine(line: WireLine): string {
  const stamp = `${DIM}${formatWireTime(line.atMs)}${RESET}`
  const peer = `${PEER}${shortPeer(line.peer)}${RESET}`
  const size = line.bytes > 0 ? ` ${DIM}${formatWireBytes(line.bytes)}${RESET}` : ''
  switch (line.direction) {
    case 'sent':
      return `${stamp} ${SENT}→${RESET} ${peer}  ${WHITE}${line.label}${RESET}${size}`
    case 'received':
      return `${stamp} ${RECEIVED}←${RESET} ${peer}  ${WHITE}${line.label}${RESET}${size}`
    default:
      return `${stamp} ${NOTE}•${RESET} ${peer}  ${NOTE_TEXT}${line.label}${RESET}`
  }
}