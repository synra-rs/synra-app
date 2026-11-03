export function isComposing(event: KeyboardEvent): boolean {
  return event.isComposing || event.keyCode === 229
}

/** Let Escape cancel the input method without dismissing its containing dialog. */
export function guardCompositionEscape(event: KeyboardEvent): void {
  if (event.key === 'Escape' && isComposing(event))
    event.stopPropagation()
}
