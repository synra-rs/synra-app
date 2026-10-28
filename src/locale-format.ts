import { computed } from 'vue'
import { locale } from './locale'

const dates = computed(() => new Intl.DateTimeFormat(locale.value, {
  year: 'numeric',
  month: 'numeric',
  day: 'numeric',
  hour: 'numeric',
  minute: 'numeric',
  second: 'numeric'
}))
const times = computed(() => new Intl.DateTimeFormat(locale.value, { hour: 'numeric', minute: 'numeric', second: 'numeric' }))
const shortTimes = computed(() => new Intl.DateTimeFormat(locale.value, { hour: '2-digit', minute: '2-digit' }))

/** Cached per language, rather than constructing an Intl formatter for every table row. */
export function formatDateTime(at: number): string {
  return dates.value.format(at)
}

export function formatTime(at: number, short = false): string {
  return (short ? shortTimes : times).value.format(at)
}
