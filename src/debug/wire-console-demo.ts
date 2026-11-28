import type { WireLine } from './wire'

export function demoWireLines(base: number): WireLine[] {
  return [
    {
      atMs: base,
      direction: 'note',
      peer: 'local',
      label: 'scan started · 3000ms',
      bytes: 0
    },
    {
      atMs: base + 40,
      direction: 'note',
      peer: 'lan',
      label: 'query',
      bytes: 0
    },
    {
      atMs: base + 300,
      direction: 'note',
      peer: 'lan',
      label: 'heard 192.168.0.192:3413',
      bytes: 0
    },
    {
      atMs: base + 320,
      direction: 'received',
      peer: '7f3a9c21',
      label: '_synra.device.changed',
      bytes: 214
    },
    {
      atMs: base + 900,
      direction: 'note',
      peer: '7f3a9c21',
      label: 'dial 192.168.0.42:47100',
      bytes: 0
    },
    {
      atMs: base + 980,
      direction: 'sent',
      peer: '7f3a9c21',
      label: '_synra.pair.request',
      bytes: 128
    },
    {
      atMs: base + 1280,
      direction: 'received',
      peer: '7f3a9c21',
      label: '_synra.pair.response',
      bytes: 42
    },
    {
      atMs: base + 1300,
      direction: 'note',
      peer: '7f3a9c21',
      label: 'paired · accepted',
      bytes: 0
    },
    {
      atMs: base + 4100,
      direction: 'sent',
      peer: '7f3a9c21',
      label: '_synra.session.hello',
      bytes: 96
    },
    {
      atMs: base + 4300,
      direction: 'received',
      peer: '7f3a9c21',
      label: '_synra.plugin.list',
      bytes: 2048
    },
    {
      atMs: base + 5100,
      direction: 'sent',
      peer: '7f3a9c21',
      label: 'chunk #7',
      bytes: 65_536
    },
    {
      atMs: base + 5120,
      direction: 'received',
      peer: '7f3a9c21',
      label: '_synra.transfer.accept',
      bytes: 64
    }
  ]
}
