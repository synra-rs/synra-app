import type { IpcError } from '@synra/protocol'
import fixture from '../fixtures/registry-contract.json'

export const registryFixture = fixture as {
  notConfigured: IpcError
  invalidConfiguration: IpcError
  searchUnsupported: IpcError
  network: IpcError
  settings: {
    present: boolean
    value: unknown
    outcome: { error?: IpcError, sources?: { kind: 'npm' | 'static', url: string }[] }
  }[]
}
