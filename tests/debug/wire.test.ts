import { describe, expect, it } from 'vitest'

import { formatWireBytes, formatWireLine, formatWireTime, shortPeer } from '@/debug/wire'

function line(overrides: Partial<Parameters<typeof formatWireLine>[0]> = {}) {
  return {
    atMs: new Date(2026, 10, 2, 21, 5, 7, 42).getTime(),
    direction: 'sent',
    peer: 'dev_b',
    label: '_synra.pair.request',
    bytes: 1536,
    ...overrides
  }
}

describe('the wire formatter', () => {
  it('stamps a line with milliseconds, because two frames in one second are ordinary', () => {
    expect(formatWireTime(line().atMs)).toBe('21:05:07.042')
  })

  it('marks a sent frame with a cyan arrow and its size', () => {
    const text = formatWireLine(line())
    expect(text).toContain('\x1B[38;5;45m→\x1B[0m')
    expect(text).toContain('_synra.pair.request')
    expect(text).toContain('1.5KB')
    expect(text).toContain('\x1B[90m')
  })

  it('marks a received frame with a green arrow', () => {
    const text = formatWireLine(line({ direction: 'received' }))
    expect(text).toContain('\x1B[38;5;120m←\x1B[0m')
  })

  it('marks a note with an amber dot and no size', () => {
    const text = formatWireLine(line({ direction: 'note', bytes: 0, label: 'dial failed' }))
    expect(text).toContain('\x1B[38;5;220m•\x1B[0m')
    expect(text).toContain('dial failed')
    expect(text).not.toContain('B')
  })

  it('shortens a device identifier so the event name stays readable', () => {
    expect(shortPeer('0123456789abcdef')).toBe('01234567…')
    expect(shortPeer('local')).toBe('local')
    expect(shortPeer('dev_b')).toBe('dev_b')
  })

  it('scales bytes at the two boundaries a reader cares about', () => {
    expect(formatWireBytes(0)).toBe('0B')
    expect(formatWireBytes(1023)).toBe('1023B')
    expect(formatWireBytes(1024)).toBe('1.0KB')
    expect(formatWireBytes(1024 * 1024)).toBe('1.0MB')
  })
})
