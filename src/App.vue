<style>
.screen {
  background-image: radial-gradient(58rem 40rem at 24% -32%, color-mix(in oklab, var(--synra-primary) 10%, transparent), transparent);
  background-attachment: fixed;
}
</style>

<template>
  <ConfigProvider :theme="componentTheme" :locale="componentLocale" :auto-insert-space-in-button="false">
    <TransferPrompt />
    <PairingPrompt />
    <InputPrompt />
    <DebugConsole />
    <SearchPalette ref="searchPalette" />
    <div class="fixed inset-0 grid grid-rows-[minmax(0,1fr)_auto] overflow-hidden screen md:grid-rows-[minmax(0,1fr)]" :class="rail ? 'md:grid-cols-[6rem_minmax(0,1fr)]' : 'md:grid-cols-[15rem_minmax(0,1fr)]'">
      <AppNavigation :rail="rail" @search="searchPalette?.show()" />
      <main ref="contentPane" tabindex="0" :aria-label="messages.app.content" class="[scrollbar-gutter:stable] min-h-0 min-w-0 overflow-auto overscroll-contain px-4 py-5 text-base focusable lg:px-10 md:px-8 sm:px-6 focus-visible:ring-inset">
        <div class="mx-auto max-w-[1200px] min-w-0 w-full">
          <div v-if="!online" role="alert" class="mb-4 border border-line rounded-surface bg-raised px-4 py-3 text-base">
            <span class="font-medium">{{ messages.app.offline }}</span>
            <span class="text-ink-dim"> {{ messages.app.offlineDetail }}</span>
          </div>
          <CrashNotice />
          <NoticeHost />
          <RouterView />
        </div>
      </main>
      <AppNavigation bottom :rail="rail" />
    </div>
  </ConfigProvider>
</template>

<script setup lang="ts">
import type { DevicesSnapshot } from '@synra/protocol'
import { synraAntdTheme } from '@synra/theme/antd'
import { ConfigProvider } from 'ant-design-vue'
import enUS from 'ant-design-vue/es/locale/en_US'
import zhCN from 'ant-design-vue/es/locale/zh_CN'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import AppNavigation from './components/AppNavigation.vue'
import CrashNotice from './components/CrashNotice.vue'
import DebugConsole from './components/DebugConsole.vue'
import InputPrompt from './components/InputPrompt.vue'
import NoticeHost from './components/NoticeHost.vue'
import PairingPrompt from './components/PairingPrompt.vue'
import SearchPalette from './components/SearchPalette.vue'
import TransferPrompt from './components/TransferPrompt.vue'
import { kernel } from './kernel'
import { locale, watchSystemLocale } from './locale'
import { messages } from './messages'
import { useNetworkStatus } from './network'
import { useDevicesStore } from './stores/devices'
import { useSettingsStore } from './stores/settings'

const route = useRoute()
const devices = useDevicesStore()
const settings = useSettingsStore()
const componentTheme = synraAntdTheme()
const componentLocale = computed(() => locale.value === 'zh-CN' ? zhCN : enUS)
const online = useNetworkStatus()
const contentPane = ref<HTMLElement>()
const searchPalette = ref<{ show: () => void }>()
const rail = ref(false)
let query: MediaQueryList | undefined
let stopLocaleSync = (): void => {}

watch(locale, value => document.documentElement.lang = value, { immediate: true })
watch(() => route.path, () => contentPane.value?.scrollTo({ top: 0, left: 0 }), { flush: 'post' })

function apply(event: MediaQueryList | MediaQueryListEvent): void {
  rail.value = !event.matches
}

onMounted(async () => {
  stopLocaleSync = watchSystemLocale()
  query = window.matchMedia('(min-width: 64rem)')
  apply(query)
  query.addEventListener('change', apply)
  const [answer] = await Promise.all([kernel.call<DevicesSnapshot>('devices_snapshot'), settings.read(kernel)])
  if (answer.ok)
    devices.applySnapshot(answer.value)
})

onUnmounted(() => {
  stopLocaleSync()
  query?.removeEventListener('change', apply)
})
</script>
