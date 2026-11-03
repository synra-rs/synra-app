/** Browser observers used by mounted Ant Design controls. Install for every test. */
export function installBrowserApis(): void {
  window.matchMedia = (media: string) => ({ matches: false, media, onchange: null, addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent: () => false }) as MediaQueryList
  const observer = class {
    observe() {}
    unobserve() {}
    disconnect() {}
    takeRecords() { return [] }
  }
  globalThis.ResizeObserver = observer as unknown as typeof ResizeObserver
  globalThis.IntersectionObserver = observer as unknown as typeof IntersectionObserver
}
