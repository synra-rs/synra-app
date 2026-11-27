import { describe, expect, it, vi } from 'vitest'
import { createHostBackHandler, EXIT_CONFIRMATION_MS, isPrimaryRoute } from '@/app/back'

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

function controls() {
  return {
    dismissDialog: vi.fn(() => false),
    currentRoute: vi.fn(() => ({ path: '/plugins', query: {}, hash: '' })),
    hasRouterHistory: vi.fn(() => false),
    goBack: vi.fn(),
    replace: vi.fn(async () => {}),
    prompt: vi.fn(),
    exit: vi.fn(async () => {})
  }
}

describe('android system back', () => {
  it('dismisses the active dialog before using the application history', async () => {
    const actions = controls()
    const back = createHostBackHandler(actions)
    actions.hasRouterHistory.mockReturnValue(true)
    actions.dismissDialog.mockReturnValueOnce(true)
    await back(true)
    expect(actions.goBack).not.toHaveBeenCalled()
    await back(true)
    expect(actions.goBack).toHaveBeenCalledOnce()
    expect(actions.prompt).not.toHaveBeenCalled()
  })

  it('returns a direct query/hash link to its base without using external webview history', async () => {
    const actions = controls()
    const back = createHostBackHandler(actions)
    actions.currentRoute.mockReturnValue({
      path: '/plugins',
      query: { plugin: '@acme/tool' },
      hash: '#details'
    })
    await back(true)
    expect(actions.replace).toHaveBeenCalledWith('/plugins')
    expect(actions.goBack).not.toHaveBeenCalled()
    expect(actions.prompt).not.toHaveBeenCalled()
  })

  it('returns an unknown direct route to home', async () => {
    const actions = controls()
    const back = createHostBackHandler(actions)
    actions.currentRoute.mockReturnValue({ path: '/missing', query: {}, hash: '' })
    await back(false)
    expect(actions.replace).toHaveBeenCalledWith('/')
  })

  it('uses one confirmation window and sends only one in-flight exit', async () => {
    const actions = controls()
    let now = 0
    const back = createHostBackHandler(actions, () => now)
    let finishExit: () => void = () => {}
    actions.exit.mockImplementation(() => new Promise<void>((resolve) => {
      finishExit = resolve
    }))

    await back(false)
    now += EXIT_CONFIRMATION_MS
    await back(false)
    expect(actions.prompt).toHaveBeenCalledTimes(2)
    expect(actions.exit).not.toHaveBeenCalled()
    now += EXIT_CONFIRMATION_MS - 1
    const exiting = back(false)
    await back(false)
    expect(actions.exit).toHaveBeenCalledOnce()
    finishExit()
    await exiting
  })
})
