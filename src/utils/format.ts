/**
 * One place that turns bytes into something a person reads.
 *
 * ★ Extracted when the plugin page needed the same sentence as the home page's transfer bar. Two copies would round the
 * same number differently the first time one of them changed, and a size is exactly the kind of value users compare
 * across screens.
 */

/** A byte count a person can read. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024)
    return `${bytes} B`
  const units = ['KiB', 'MiB', 'GiB', 'TiB']
  let value = bytes / 1024
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${value.toFixed(1)} ${units[unit]}`
}
