import { describe, expect, it, vi } from 'vitest'
import { effectScope } from 'vue'
import { usePluginOperations } from './operations'

describe('plugin operation ownership', () => {
  it('serializes conflicting commands per plugin while other plugins remain independent', async () => {
    const scope = effectScope()
    const operations = scope.run(usePluginOperations)!
    let finish!: (value: string) => void
    const pending = new Promise<string>((resolve) => {
      finish = resolve
    })
    try {
      const first = operations.run('one', 'update', () => pending)
      const duplicate = vi.fn(async () => 'removed')
      expect(await operations.run('one', 'remove', duplicate)).toBeUndefined()
      expect(duplicate).not.toHaveBeenCalled()
      expect(await operations.run('two', 'check', async () => 'checked')).toBe('checked')
      expect(operations.busy.get('one')).toBe('update')
      finish('updated')
      expect(await first).toBe('updated')
      expect(operations.busy.size).toBe(0)
    }
    finally {
      scope.stop()
    }
  })

  it('releases an operation after rejection and suppresses late UI results after disposal', async () => {
    const scope = effectScope()
    const operations = scope.run(usePluginOperations)!
    await expect(operations.run('one', 'open', async () => {
      throw new Error('bridge unavailable')
    })).rejects.toThrow('bridge unavailable')
    expect(operations.busy.size).toBe(0)
    let finish!: (value: string) => void
    const pending = operations.run('one', 'check', () => new Promise<string>((resolve) => {
      finish = resolve
    }))
    scope.stop()
    finish('late')
    expect(await pending).toBeUndefined()
    const command = vi.fn(async () => 'unreachable')
    expect(await operations.run('two', 'remove', command)).toBeUndefined()
    expect(command).not.toHaveBeenCalled()
  })
})
