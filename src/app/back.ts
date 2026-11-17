/** A primary page has no nested query or hash state left to leave first. */
export function isPrimaryRoute(path: string, query: Readonly<Record<string, unknown>>, hash: string): boolean {
  return (path === '/' || path === '/devices' || path === '/plugins' || path === '/settings')
    && Object.keys(query).length === 0
    && hash === ''
}
