<style>
/*
 * ★ The page wash. This is the one place it can live.
 *
 * Three routes were tried and all three failed silently — no build error, no visible change:
 *   1. `bg-[radial-gradient(a,b),radial-gradient(c,d)]` in the `screen` shortcut — UnoCSS drops the arbitrary value
 *      because of its commas and parentheses.
 *   2. `bg-[image:var(--x)]` in the same place — the form does not generate at all.
 *   3. A hand-written `.page-glow` class **named inside the shortcut** — UnoCSS reports
 *      `unmatched utility "page-glow" in shortcut "screen"` and discards it, because every entry in a shortcut is
 *      resolved as a utility. A hand-written class can never be listed there.
 *
 * `resetCss()` in `@synra/theme/css` does carry this, but that function is exported and never imported by this app —
 * `main.ts` pulls `uno.css` only — so editing it changes nothing on screen either.
 *
 * The rule targets `.screen`, the shortcut the shell root already uses, so no template change is needed.
 *
 * Both stops are mixed from `--synra-primary`, not white: a white lift greys the dark surface out, and every contrast
 * pair in `convention.ts` is measured against `--synra-screen`. `fixed` keeps the light anchored while a page scrolls.
 */
.screen {
  background-image:
    radial-gradient(60rem 40rem at 12% -10%, color-mix(in oklab, var(--synra-primary) 16%, transparent), transparent),
    radial-gradient(50rem 36rem at 100% 100%, color-mix(in oklab, var(--synra-primary) 10%, transparent), transparent);
  background-attachment: fixed;
}
</style>

<template>
  <!--
    ★ The one place where the tokens meet the component library (R51).

    Every control from here down is `ant-design-vue`, and every colour, radius, font and duration it uses comes from
    `synraAntdTheme()` — which is a mapping of `@synra/theme`'s tokens, not a second set of values. A second theme object
    anywhere in the interface is the failure this line exists to prevent, so there is exactly one `:theme=` in the app.
  -->
  <ConfigProvider :theme="componentTheme" :locale="componentLocale">
    <!-- Constrain the frame, so content scrolls inside its pane instead of growing the document. -->
    <div class="fixed inset-0 grid grid-rows-[minmax(0,1fr)_auto] overflow-hidden screen md:grid-rows-[minmax(0,1fr)]" :class="rail ? 'md:grid-cols-[6rem_minmax(0,1fr)]' : 'md:grid-cols-[15rem_minmax(0,1fr)]'">
      <!-- The one prompt that is about a peer waiting on this answer; at the root so it is visible on every page. -->
      <TransferPrompt />
      <!-- The shell's search: the entry is in the sidebar, the box is a modal at the root. -->
      <SearchPalette ref="searchPalette" />
      <!--
        The navigation, in the three forms R11 gives it: an expanded sidebar at `lg` (which is also `minWidth`, R12), an
        icon rail at `md`, and nothing at all below that — where the bottom bar takes over.

        ★ **It is `a-menu`, not a hand-built list.** R51 names menus among the things the component library owns, and the
        library's `inline` mode with `inline-collapsed` *is* the expanded/rail pair: it brings the collapsed width, the
        icon alignment, the tooltip that names a collapsed item, and the selected-item treatment. The first version of
        this file hand-rolled all of that, which is how the sidebar ended up as the least antd-looking part of an antd
        application. The one thing that stays hand-built is the bottom bar below `md`: antd has no bottom navigation, and
        three forms for three widths is a product decision rather than a component one.

        ★ **Its own inline `border-inline-end` stays.** The first attempt killed it with `!border-e-0` — and the theme's
        `blocklist` refused the build, which is the rule working exactly as designed (`!important` is banned there by
        name: once one is allowed, the next has to be louder). antd derives that border from `colorSplit`, which the
        adapter maps to `line`, so what is beside the shell's own `border-line` edge is a token-coloured hairline, not a
        foreign colour. Two hairlines is a cosmetic debt worth naming, not worth a second theme object (R51).
      -->
      <aside class="hidden min-h-0 min-w-0 flex-col overflow-hidden border-e border-line bg-glass px-2 py-3 md:flex">
        <div class="row shrink-0 gap-2 px-2 pb-3" :class="{ 'justify-center': rail }">
          <span class="i-carbon-connect text-lg text-primary" aria-hidden="true" />
          <span v-if="!rail" class="text-lg font-semibold">Synra</span>
          <!--
            ★ The search entry lives beside the wordmark rather than in the page header, because it is the shell's and
              not a page's: the same box on four pages would be four entries to keep in step. It stays in the rail form
              too — this is the one control that is *more* useful when the sidebar is narrow.

            ★ The `/` is shown, not described. A shortcut nobody can see is a shortcut only its author uses, and it
              costs one character here.
          -->
          <!--
            ★ A ghost icon, not a bordered field. The earlier version drew a border, a fill and a placeholder so it
              would "look like a search box" — and a border plus a fill around one glyph is what makes a control look
              raised. There is no text to type into here; it opens one, so it should look like a button, and the
              lightest button in the system is an icon with nothing behind it until the pointer arrives.

            ★ The shortcut is a tooltip, not a label. `/` beside the glyph made the glyph read as "search slash"; in a
              tooltip it answers the question a person actually has on hover — "how do I get here faster".
          -->
          <ATooltip placement="right">
            <template #title>
              {{ messages.search.shortcutHint }}<code class="rounded-control bg-raised px-1 py-0.5 font-mono">{{ messages.search.shortcut }}</code>
            </template>
            <!--
              ★ No `focusable` here, and that is the fix this control needed twice. `focusable` is the theme's
                always-on ring — right for a menu item, wrong for an icon: it drew a border and a fill around a single
                glyph, which is exactly the "embossed button" this kept looking like. A ghost icon wants the ring only
                on focus, which `focus-visible` provides.

              ★ `bg-transparent` is not redundant with "no background class". The element is a `<button>`, and the
                user-agent sheet gives it `background-color: buttonface` plus `border: 1px solid buttonborder` — the
                grey tile that survived three attempts at restyling. `border-0` and `bg-transparent` are what actually
                turn it into a ghost; without them nothing the class list says can win.

              ★ 44px tall (`09 §3`), only as wide as the glyph plus padding, and `rounded-pill`: a 44 × 44 tile with
                near-flat corners is what reads as raised at this size, whatever is painted behind it.
            -->
            <button
              type="button"
              class="ms-auto grid min-h-11 shrink-0 place-items-center rounded-pill border-0 bg-transparent px-2.5 text-ink-dim transition-colors hover:bg-raised hover:text-ink focus-visible:ring-1 focus-visible:ring-primary md:min-h-8"
              :class="{ 'ms-0': rail }"
              :aria-label="messages.search.title"
              data-testid="search-entry"
              @click="searchPalette?.show()"
            >
              <span class="i-carbon-search text-base" aria-hidden="true" />
            </button>
          </ATooltip>
        </div>

        <!--
          ★ **The items carry the focus ring, and it took a browser to notice they did not.**

            `a-menu` sets `outline: none` on its items and never replaces it, so a person tabbing through the sidebar
            had no idea where they were — the exact failure `@synra/theme`'s `focusable` shortcut exists to prevent
            ("`outline-none` without a replacement is the easiest accessibility mistake in this interface"). R35 says
            the `focus-visible` ring is always present, so this is not a taste question.

            It is written as an arbitrary variant on the *menu* rather than a class on each `a-menu-item`, because the
            item is the library's element and its own selector is what has to change. `@synra/theme`'s ring is reused
            by name (`ring-2` + `ring-primary-ink` + `ring-offset-screen`) instead of restating a colour.
        -->
        <AMenu
          :aria-label="messages.app.navigation"
          class="min-h-0 flex-1 overflow-y-auto overscroll-contain [&_.ant-menu-item:focus-visible]:ring-2 [&_.ant-menu-item:focus-visible]:ring-offset-2 [&_.ant-menu-item:focus-visible]:ring-primary-ink [&_.ant-menu-item:focus-visible]:ring-offset-screen"
          mode="inline"
          :inline-collapsed="rail"
          :selected-keys="[route.path]"
          @click="navigate"
        >
          <AMenuItem v-for="page in PAGES" :key="page.path">
            <template #icon>
              <span :class="page.icon" aria-hidden="true" />
            </template>
            {{ page.label }}
          </AMenuItem>
        </AMenu>

        <!--
          Which device this is. In the shell rather than on each page, because the question is answered the same way
          everywhere (see the script's note) — and at the bottom because it is the one thing on screen that never
          changes while you navigate (R25: this border separates two kinds of information, so it is a border worth
          having).
        -->
        <div class="mt-auto min-w-0 flex shrink-0 flex-col gap-2 border-t border-line pt-3" :class="rail ? 'items-center' : 'px-2'">
          <template v-if="devices.identity">
            <div class="row gap-2">
              <AAvatar :size="28" class="shrink-0 bg-raised text-primary">
                <span class="i-carbon-laptop" aria-hidden="true" />
              </AAvatar>
              <span v-if="!rail" class="min-w-0 flex flex-col">
                <span class="truncate text-sm font-medium">{{ devices.identity.displayName }}</span>
                <span class="truncate text-xs text-ink-dim">{{ devices.identity.platform }}</span>
              </span>
            </div>
            <span v-if="!rail" class="truncate text-xs text-ink-dim font-mono">v{{ devices.identity.kernelVersion }}</span>
          </template>
          <!--
            A real skeleton rather than the word "loading": the interface is starting up, and "loading…" is a sentence
            that says nothing a pair of dim bars does not say faster. Not `active` — R29 spends a screen's one motion on
            something that answers a question, and a shimmer answers none.
          -->
          <ASkeleton v-else :title="false" :paragraph="{ rows: rail ? 1 : 2 }" />
        </div>
      </aside>

      <!--
        ★ `aria-label` because the region is focusable. A focusable region with no name is announced by its entire
        contents: `bsk_inspect.observe` rendered this element as `main "首页 设备连接 已连接 This device Pixel 8 …"`,
        so the label a screen reader gives the scroll container was the first dozen words of whatever page was open.
      -->
      <main ref="contentPane" tabindex="0" :aria-label="messages.app.content" class="[scrollbar-gutter:stable] min-h-0 min-w-0 overflow-auto overscroll-contain px-4 py-5 text-sm focusable lg:px-10 md:px-8 sm:px-6 focus-visible:ring-inset">
        <!--
          ★ The shell centres the content region but does not decide how wide a page should be.

          It used to carry `max-w-5xl`, which meant every page was capped at 1024px no matter what it was doing. On a
          1904px window that left roughly 400px of empty ground on each side of a plugin list — the "content marooned
          in the middle of a wide screen" the quality bar names as a demo-feeling source — and it silently overrode the
          width each page actually needs. [03 §3](../../design-docs/ui-design/03-shell-and-navigation.md) says the
          opposite: a page declares whether it is a *reading/form* width or a *workspace* width, and the shell does not
          guess from page DOM.

          So the shell keeps only a guard rail (`max-w-[1200px]`, the workspace upper bound that section gives) and each
          page narrows itself. A page that wants the old single-column reading measure asks for it; one that wants a
          workspace gets the room back.
        -->
        <div class="mx-auto max-w-[1200px] min-w-0 w-full">
          <!--
            ★ The one global fact that changes what every page can do. A device row can say "not reachable", but that is
            about one peer; this is about the machine, and every sharing path needs it.
          -->
          <div
            v-if="!online"
            role="alert"
            class="mb-4 border border-line rounded-lg bg-raised px-4 py-3 text-sm"
          >
            <span class="font-medium">{{ messages.app.offline }}</span>
            <span class="text-ink-dim"> {{ messages.app.offlineDetail }}</span>
          </div>
          <CrashNotice />
          <NoticeHost />
          <RouterView />
        </div>
      </main>

      <!--
        The bottom bar, below `md` only — the third form, and the one antd has no component for. `min-h-11` is R16's
        44px touch target, and the padding is the safe-area inset that `viewport-fit=cover` in `index.html` makes
        non-zero on a phone with a gesture bar.
      -->
      <nav class="flex border-t border-line bg-glass pb-[env(safe-area-inset-bottom)] md:hidden">
        <RouterLink
          v-for="page in PAGES"
          :key="page.path"
          v-slot="{ isExactActive }"
          :to="page.path"
          class="relative min-h-11 row flex-1 flex-col justify-center gap-0.5 text-xs text-ink-dim focusable @hover:text-ink"
          active-class="text-primary-ink"
        >
          <span
            class="absolute inset-x-3 top-0 h-0.5 rounded-pill bg-transparent"
            :class="{ 'bg-primary': isExactActive }"
            aria-hidden="true"
          />
          <span :class="page.icon" class="text-lg" aria-hidden="true" />
          <span>{{ page.label }}</span>
        </RouterLink>
      </nav>
    </div>
  </ConfigProvider>
</template>

<script setup lang="ts">
/**
 * The frame every page renders inside: navigation on the left (or the bottom), the page on the right.
 *
 * # Why the identity is in the shell and not on each page
 *
 * "Which device am I looking at" is answered the same way everywhere, so answering it in four places is four chances to
 * answer it differently. The kernel's identity snapshot is fetched once, here, and the pages that need it read the store.
 *
 * # What the shell owns, and what it does not
 *
 * It owns the three navigation forms, the seam between pages, and — because it is the root — the **one** place where the
 * design tokens are handed to the component library (`ConfigProvider`, R51). It owns **no colour, size or shadow of its
 * own**: the classes below are names from `@synra/theme` (`screen`, `floating`, `focusable`, `bg-raised`, `border-line`),
 * so a change to the palette is a change in one repository. There is no `<style>` block at all — R37 allows one only
 * where UnoCSS cannot express the thing, and nothing here qualifies.
 *
 * # Why the rail width is decided in script and not in CSS
 *
 * `a-menu` collapses by a **prop** (`inline-collapsed`), not by a media query, so the shell has to know which of the two
 * forms it is in. `matchMedia` answers that once per crossing rather than on every render, and the listener is removed
 * with the component. The breakpoint is 64rem — which is R12's `minWidth` (1024px) and `lg`, so "there is room for the
 * expanded form" and "the window is at least the width the design promises" are the same sentence. Below `md` (48rem)
 * the sidebar is hidden entirely and the bottom bar is the navigation, so the two answers never disagree about what is on
 * screen.
 */
import type { DevicesSnapshot } from '@synra/protocol'
import { synraAntdTheme } from '@synra/theme/antd'
import { Avatar as AAvatar, Menu as AMenu, MenuItem as AMenuItem, Skeleton as ASkeleton, Tooltip as ATooltip, ConfigProvider } from 'ant-design-vue'
import enUS from 'ant-design-vue/es/locale/en_US'
import zhCN from 'ant-design-vue/es/locale/zh_CN'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'

import CrashNotice from './components/CrashNotice.vue'
import NoticeHost from './components/NoticeHost.vue'
import SearchPalette from './components/SearchPalette.vue'
import TransferPrompt from './components/TransferPrompt.vue'
import { kernel } from './kernel'
import { locale, watchSystemLocale } from './locale'
import { messages } from './messages'
import { useNetworkStatus } from './network'
import { PAGES } from './router'
import { useDevicesStore } from './stores/devices'
import { useSettingsStore } from './stores/settings'

const route = useRoute()
const router = useRouter()
const devices = useDevicesStore()
const settings = useSettingsStore()
const componentTheme = synraAntdTheme()
const componentLocale = computed(() => locale.value === 'zh-CN' ? zhCN : enUS)
let stopLocaleSync = (): void => {}

watch(locale, (value) => {
  document.documentElement.lang = value
}, { immediate: true })
const online = useNetworkStatus()
const contentPane = ref<HTMLElement>()
/** The search box, opened by its sidebar entry. The `/` shortcut is the component's own listener. */
const searchPalette = ref<{ show: () => void }>()

// Route content shares one scroll pane; each new page starts at its own beginning.
watch(() => route.path, () => contentPane.value?.scrollTo({ top: 0, left: 0 }), { flush: 'post' })

/** True in the rail form: below `lg` (64rem), which is also R12's `minWidth` floor for the expanded form. */
const rail = ref(false)
let query: MediaQueryList | undefined

function apply(event: MediaQueryList | MediaQueryListEvent): void {
  rail.value = !event.matches
}

function navigate(info: { key: string | number }): void {
  router.push(String(info.key))
}

onMounted(async () => {
  stopLocaleSync = watchSystemLocale()
  query = window.matchMedia('(min-width: 64rem)')
  apply(query)
  query.addEventListener('change', apply)

  // The snapshot is taken before anything subscribes, which is the wrong order in general and the right one *here*:
  // the interface is starting up with nothing on screen, so there is no event it can miss yet. Once a page is live the
  // dispatcher owns the ordering.
  const [answer] = await Promise.all([kernel.call<DevicesSnapshot>('devices_snapshot'), settings.read(kernel)])
  if (answer.ok)
    devices.applySnapshot(answer.value)
})

onUnmounted(() => {
  stopLocaleSync()
  query?.removeEventListener('change', apply)
})
</script>
