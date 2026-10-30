/**
 * `formatBytes` is the only place a byte count becomes words, so its units are what a person compares across screens.
 *
 * ★ These cases exist because the plugin page once divided by 1 MiB inline and pasted the result into a template that
 * said "MiB" — so this repository's own 87 KB starter package read "About 0.1 MiB". The unit has to follow the
 * magnitude, and below a kilobyte no unit is more honest than the count itself.
 */
import { describe, expect, it } from 'vitest'

import { formatBytes } from './format'

describe('formatBytes', () => {
  it.each([
    [0, '0 B'],
    [1, '1 B'],
    [512, '512 B'],
    [1023, '1023 B'],
    [1024, '1.0 KiB'],
    [87 * 1024, '87.0 KiB'],
    [2048, '2.0 KiB'],
    [1024 * 1024, '1.0 MiB'],
    [64 * 1024 * 1024, '64.0 MiB'],
    [1024 * 1024 * 1024, '1.0 GiB']
  ])('renders %i as %s', (bytes, expected) => {
    expect(formatBytes(bytes)).toBe(expected)
  })

  it('never rounds a real size down to zero', () => {
    // The failure the inline division produced: any package under 1 MiB became "0.0". A count under 1024 keeps its
    // bytes rather than being divided into a fraction of a larger unit.
    expect(formatBytes(512)).not.toContain('0.0')
    expect(formatBytes(512)).toBe('512 B')
  })
})
