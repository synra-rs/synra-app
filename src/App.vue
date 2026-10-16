<template>
  <!--
    ★ The one place where the tokens meet the component library (R51).

    Every control from here down is `ant-design-vue`, and every colour, radius, font and duration it uses comes from
    `synraAntdTheme()` — which is a mapping of `@synra/theme`'s tokens, not a second set of values. A second theme object
    anywhere in the interface is the failure this line exists to prevent, so there is exactly one `:theme=` in the app.
  -->
  <ConfigProvider :theme="synraAntdTheme()">
    <div class="grid grid-rows-[1fr_auto] screen lg:grid-cols-[15rem_1fr] md:grid-cols-[4.5rem_1fr] md:grid-rows-1">
      <!--
        The navigation, in the three forms R11 gives it: an expanded sidebar at `lg` (which is also `minWidth`, R12),
        an icon rail at `md`, and nothing at all below that — where the bottom bar takes over.
      -->
      <aside class="hidden flex-col gap-4 border-e border-line bg-surface px-2 py-3 md:flex lg:px-4">
        <div class="row gap-2 ps-2">
          <span class="i-carbon-connect text-lg text-primary" aria-hidden="true" />
          <span class="hidden text-lg font-semibold lg:inline">Synra</span>
        </div>

        <nav class="min-h-0 stack gap-1 overflow-y-auto">
          <RouterLink
            v-for="page in PAGES"
            :key="page.path"
            v-slot="{ isExactActive }"
            :to="page.path"
            class="relative row gap-3 rounded-control px-2 py-2 text-ink-dim focusable @hover:bg-raised lg:px-3 @hover:text-ink"
            active-class="bg-raised text-ink"
          >
            <!--
              The current page is marked twice: a bar and a colour. Colour alone is not a state (R23) — and the bar is
              what survives a screenshot in greyscale.
            -->
            <span
              class="absolute start-0 inset-y-2 w-0.5 rounded-pill bg-transparent"
              :class="{ 'bg-primary': isExactActive }"
              aria-hidden="true"
            />
            <span :class="page.icon" class="shrink-0 text-lg" aria-hidden="true" />
            <span class="hidden min-w-0 flex-col lg:flex">
              <span class="truncate font-medium">{{ page.label }}</span>
              <span class="truncate text-xs">{{ page.hint }}</span>
            </span>
          </RouterLink>
        </nav>

        <!--
          Which device this is. In the shell rather than on each page, because the question is answered the same way
          everywhere (see the script's note) — and at the bottom because it is the one thing on screen that never
          changes while you navigate (R25: this border separates two kinds of information, so it is a border worth
          having).
        -->
        <div class="mt-auto hidden flex-col gap-0.5 border-t border-line px-2 pt-3 lg:flex">
          <template v-if="devices.identity">
            <span class="truncate text-sm font-medium">{{ devices.identity.displayName }}</span>
            <span class="truncate text-xs">{{ devices.identity.platform }}</span>
            <span class="truncate text-xs font-mono">v{{ devices.identity.kernelVersion }}</span>
          </template>
          <!--
            A skeleton rather than the word "loading": the interface is starting up, and "loading…" is a sentence that
            says nothing a pair of dim bars does not say faster. No animation — R29 spends a screen's one motion on
            something that answers a question, and this shapes does not.
          -->
          <template v-else>
            <span class="h-4 w-24 rounded-pill bg-raised" aria-hidden="true" />
            <span class="h-3 w-16 rounded-pill bg-raised" aria-hidden="true" />
          </template>
        </div>
      </aside>

      <main class="min-h-0 overflow-y-auto px-4 py-5 text-base lg:px-10 md:px-8 sm:px-6">
        <div class="mx-auto max-w-3xl w-full">
          <RouterView />
        </div>
      </main>

      <!--
        The bottom bar, below `md` only. `min-h-11` is R16's 44px touch target, and the padding is the safe-area inset
        that `viewport-fit=cover` in `index.html` makes non-zero on a phone with a gesture bar.
      -->
      <nav class="flex border-t border-line bg-surface pb-[env(safe-area-inset-bottom)] md:hidden">
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
 * own**: the classes below are names from `@synra/theme` (`screen`, `panel`, `focusable`, `bg-raised`, `border-line`), so
 * a change to the palette is a change in one repository. There is no `<style>` block at all — R37 allows one only where
 * UnoCSS cannot express the thing, and nothing here qualifies.
 */
import type { DevicesSnapshot } from '@synra/protocol'
import { synraAntdTheme } from '@synra/theme/antd'
import { ConfigProvider } from 'ant-design-vue'
import { onMounted } from 'vue'

import { kernel } from './kernel'
import { PAGES } from './router'
import { useDevicesStore } from './stores/devices'

const devices = useDevicesStore()

onMounted(async () => {
  // The snapshot is taken before anything subscribes, which is the wrong order in general and the right one *here*:
  // the interface is starting up with nothing on screen, so there is no event it can miss yet. Once a page is live the
  // dispatcher owns the ordering.
  const answer = await kernel.call<DevicesSnapshot>('devices_snapshot')
  if (answer.ok)
    devices.applySnapshot(answer.value)
})
</script>
