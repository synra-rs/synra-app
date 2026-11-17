import { describe, expect, it } from 'vitest'
import { isPrimaryRoute } from '@/app/back'

describe('mobile back route classification', () => {
  it('treats an unadorned page as the app root level', () => {
    expect(isPrimaryRoute('/plugins', {}, '')).toBe(true)
  })

  it('leaves nested query and hash state before exiting', () => {
    expect(isPrimaryRoute('/plugins', { plugin: '@acme/reader' }, '')).toBe(false)
    expect(isPrimaryRoute('/devices', { device: 'peer-1' }, '')).toBe(false)
    expect(isPrimaryRoute('/settings', {}, '#advanced')).toBe(false)
  })

  it('does not treat an unrelated route as a primary page', () => {
    expect(isPrimaryRoute('/missing', {}, '')).toBe(false)
  })
})
