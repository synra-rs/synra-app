<template>
  <nav v-if="bottom" :aria-label="messages.app.navigation" class="flex border-t border-line bg-glass pb-[env(safe-area-inset-bottom)] md:hidden">
    <RouterLink v-for="page in PAGES" :key="page.path" v-slot="{ isExactActive }" :to="page.path" class="relative min-h-11 row flex-1 flex-col justify-center gap-0.5 text-sm text-ink-dim focusable hover:text-ink" active-class="text-primary-ink">
      <span class="absolute inset-x-3 top-0 h-0.5 rounded-pill bg-transparent" :class="{ 'bg-primary': isExactActive }" aria-hidden="true" />
      <span :class="page.icon" class="text-lg" aria-hidden="true" />
      <span>{{ page.label }}</span>
    </RouterLink>
  </nav>
  <aside v-else class="hidden min-h-0 min-w-0 flex-col overflow-hidden border-e border-line bg-glass px-2 py-3 md:flex">
    <div class="row shrink-0 gap-2 px-2 pb-3" :class="{ 'justify-center': rail }">
      <span class="i-carbon-connect text-lg text-primary" aria-hidden="true" />
      <span v-if="!rail" class="text-lg font-semibold">Synra</span>
      <ATooltip placement="right">
        <template #title>
          {{ messages.search.shortcutHint }}<code>{{ messages.search.shortcut }}</code>
        </template>
        <AButton type="text" class="synra-icon-button ms-auto shrink-0" :class="{ 'ms-0': rail }" :aria-label="messages.search.title" data-testid="search-entry" @click="$emit('search')">
          <span class="i-carbon-search" aria-hidden="true" />
        </AButton>
      </ATooltip>
    </div>
    <AMenu :aria-label="messages.app.navigation" class="min-h-0 flex-1 overflow-y-auto overscroll-contain [&_.ant-menu-item:focus-visible]:ring-2 [&_.ant-menu-item:focus-visible]:ring-offset-2 [&_.ant-menu-item:focus-visible]:ring-primary-ink [&_.ant-menu-item:focus-visible]:ring-offset-screen" mode="inline" :inline-collapsed="rail" :selected-keys="[route.path]" @click="navigate">
      <AMenuItem v-for="page in PAGES" :key="page.path">
        <template #icon>
          <span :class="page.icon" aria-hidden="true" />
        </template>
        {{ page.label }}
      </AMenuItem>
    </AMenu>
    <div class="mt-auto min-w-0 flex shrink-0 flex-col gap-2 border-t border-line pt-3" :class="rail ? 'items-center' : 'px-2'">
      <template v-if="devices.identity">
        <div class="row gap-2">
          <AAvatar :size="28" class="shrink-0 bg-raised text-primary" :aria-label="devices.identity.displayName" :title="devices.identity.displayName">
            <span class="i-carbon-laptop" aria-hidden="true" />
          </AAvatar>
          <span v-if="!rail" class="min-w-0 flex flex-col">
            <span class="truncate text-base font-medium">{{ devices.identity.displayName }}</span>
            <span class="truncate text-sm text-ink-dim">{{ devices.identity.platform }}</span>
          </span>
        </div>
        <span v-if="!rail" class="truncate text-sm text-ink-dim font-mono">v{{ devices.identity.kernelVersion }}</span>
      </template>
      <ASkeleton v-else :title="false" :paragraph="{ rows: rail ? 1 : 2 }" />
    </div>
  </aside>
</template>

<script setup lang="ts">
import { Avatar as AAvatar, Button as AButton, Menu as AMenu, MenuItem as AMenuItem, Skeleton as ASkeleton, Tooltip as ATooltip } from 'ant-design-vue'
import { useRoute, useRouter } from 'vue-router'
import { messages } from '../messages'
import { PAGES } from '../router'
import { useDevicesStore } from '../stores/devices'

defineProps<{ rail: boolean, bottom?: boolean }>()
defineEmits<{ search: [] }>()
const route = useRoute()
const router = useRouter()
const devices = useDevicesStore()
function navigate(info: { key: string | number }): void {
  void router.push(String(info.key))
}
</script>
