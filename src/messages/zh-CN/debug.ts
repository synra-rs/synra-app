import type { LocaleEntries } from '../types'

export const debug = {
  'debug.title': '线缆',
  'debug.all': '全部',
  'debug.subtitle': '双向的每一帧',
  'debug.open': '打开线缆监视',
  'debug.close': '关闭',
  'debug.clear': '清空',
  'debug.copy': '复制',
  'debug.copied': '已复制',
  'debug.copyFailed': '复制失败',
  'debug.pause': '暂停',
  'debug.resume': '继续',
  'debug.empty': '还没有流量。扫描、配对或发送点什么。',
  'debug.sent': '发送',
  'debug.received': '接收',
  'debug.note': '标记',
  'debug.lines': (count: number) => `${count} 行`,
  'debug.hintOpen': 'Esc 关闭',
  'debug.hintClosed': '内核实时流量'
} satisfies Partial<LocaleEntries>