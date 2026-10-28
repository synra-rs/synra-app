import type { englishMessages } from './english'

type Widen<T> = T extends (...args: infer A) => string
  ? (...args: A) => string
  : T extends string ? string : { readonly [K in keyof T]: Widen<T[K]> }

export type Messages = Widen<typeof englishMessages>

type Fields<T, P extends string = ''> = {
  [K in keyof T & string]: K extends 'icon' ? never
    : T[K] extends string ? Record<`${P}${K}`, string>
      : T[K] extends (...args: infer A) => string ? Record<`${P}${K}`, (...args: A) => string>
        : Fields<T[K], `${P}${K}.`>
}[keyof T & string]

type Intersect<U> = (U extends unknown ? (value: U) => void : never) extends (value: infer I) => void ? I : never

/** Every text key is required; formatting functions retain their argument types. */
export type LocaleEntries = Intersect<Fields<typeof englishMessages>>
