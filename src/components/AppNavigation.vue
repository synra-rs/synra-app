<template>
  <nav v-if="bottom" :aria-label="messages.app.navigation" class="mobile-tabbar border-t border-line flex">
    <button type="button" class="mobile-search-entry synra-icon-button border-0 bg-transparent" :aria-label="messages.search.title" :title="messages.search.title" data-testid="mobile-search-entry" @click="emitSearch">
      <span :class="icons.search" aria-hidden="true" />
    </button>
    <button v-for="page in PAGES" :key="page.path" type="button" class="text-sm text-ink-dim focusable border-0 bg-transparent row flex-1 flex-col gap-0.5 min-h-14 justify-center relative hover:text-ink" :class="{ 'text-primary-ink': route.path === page.path }" :aria-current="route.path === page.path ? 'page' : undefined" @click="navigateTab(page.path)">
      <span class="rounded-pill bg-transparent h-0.5 inset-x-4 bottom-1 absolute" :class="{ 'bg-primary': route.path === page.path }" aria-hidden="true" />
      <span :class="page.icon" class="text-lg" aria-hidden="true" />
      <span>{{ page.label }}</span>
    </button>
  </nav>
  <aside v-else class="px-2 py-3 border-e border-line bg-glass flex flex-col min-h-0 min-w-0 overflow-hidden">
    <div class="px-2 pb-3 row shrink-0 gap-2 justify-end">
      <ATooltip placement="right">
        <template #title>
          {{ messages.search.shortcutHint }}<code>{{ messages.search.shortcut }}</code>
        </template>
        <AButton type="text" class="synra-icon-button shrink-0" :aria-label="messages.search.title" data-testid="search-entry" @click="emitSearch">
          <span :class="icons.search" aria-hidden="true" />
        </AButton>
      </ATooltip>
    </div>
    <AMenu :aria-label="messages.app.navigation" class="overscroll-contain flex-1 min-h-0 overflow-y-auto [&_.ant-menu-item:focus-visible]:ring-2 [&_.ant-menu-item:focus-visible]:ring-primary-ink [&_.ant-menu-item:focus-visible]:ring-offset-2 [&_.ant-menu-item:focus-visible]:ring-offset-screen" mode="inline" :inline-collapsed="rail" :selected-keys="[route.path]" @click="navigate">
      <AMenuItem v-for="page in PAGES" :key="page.path" :title="page.label">
        <template #icon>
          <span :class="page.icon" aria-hidden="true" />
        </template>
        {{ page.label }}
      </AMenuItem>
    </AMenu>
    <div class="mt-auto pt-3 border-t border-line flex shrink-0 flex-col gap-2 min-w-0" :class="rail ? 'items-center' : 'px-2'">
      <template v-if="devices.identity">
        <div class="row gap-2">
          <AAvatar :size="28" class="text-primary bg-raised shrink-0" :aria-label="devices.identity.displayName" :title="devices.identity.displayName">
            <span class="i-carbon-laptop" aria-hidden="true" />
          </AAvatar>
          <span v-if="!rail" class="flex flex-col min-w-0">
            <span class="text-base font-medium truncate">{{ devices.identity.displayName }}</span>
            <span class="text-sm text-ink-dim truncate">{{ devices.identity.platform }}</span>
          </span>
        </div>
        <span v-if="!rail" class="text-sm text-ink-dim font-mono truncate">v{{ devices.identity.kernelVersion }}</span>
      </template>
      <ASkeleton v-else :title="false" :paragraph="{ rows: rail ? 1 : 2 }" />
    </div>
  </aside>
</template>

<script setup lang="ts">
import { Avatar as AAvatar, Button as AButton, Menu as AMenu, MenuItem as AMenuItem, Skeleton as ASkeleton, Tooltip as ATooltip } from 'ant-design-vue'
import { useRoute, useRouter } from 'vue-router'
import { icons } from '../app/icon-map'
import { PAGES } from '../app/router'
import { messages } from '../messages'
import { useDevicesStore } from '../stores/devices'

defineProps<{ rail: boolean, bottom?: boolean }>()
const emit = defineEmits<{ search: [trigger?: HTMLElement] }>()
const route = useRoute()
const router = useRouter()
const devices = useDevicesStore()

function emitSearch(event: MouseEvent): void {
  const trigger = event.currentTarget
  emit('search', trigger instanceof HTMLElement
    ? trigger
    : undefined)
}
function navigate(info: { key: string | number }): void {
  void router.replace(String(info.key))
}
function navigateTab(path: string): void {
  void router.replace({ path, query: {}, hash: '' })
}
</script>
