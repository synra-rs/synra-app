import type { WireLine } from './wire'

const listeners = new Set<(line: WireLine) => void>()

/** The central dispatcher owns IPC; console lifetimes own only this local listener. */
export function publishWireLine(line: WireLine): void {
  for (const listener of listeners)
    listener(line)
}
export function subscribeWireLines(listener: (line: WireLine) => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}
