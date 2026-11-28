<style src="./app/shell.css"></style>

<template>
  <ConfigProvider :theme="componentTheme" :locale="componentLocale" :auto-insert-space-in-button="false" :wave="{ disabled: false }">
    <TransferPrompt />
    <PairingPrompt />
    <InputPrompt />
    <DebugConsole />
    <SearchPalette ref="searchPalette" />
    <div class="screen" :data-layout="layout.size" :data-navigation="layout.navigation" :class="{ 'mobile-host': environment?.kind === 'mobile' }">
      <WindowTitleBar v-if="environment?.kind === 'desktop'" />
      <div class="app-shell">
        <AppNavigation v-if="layout.navigation !== 'bottom'" :rail="layout.navigation === 'rail'" @search="searchPalette?.show($event)" />
        <main ref="contentPane" tabindex="0" :aria-label="messages.app.content" class="text-base px-4 py-5 focusable overscroll-contain min-h-0 min-w-0 [scrollbar-gutter:stable] overflow-auto lg:px-10 md:px-8 sm:px-6 focus-visible:ring-inset">
          <div class="mx-auto max-w-[1200px] min-w-0 w-full">
            <CrashNotice />
            <NoticeHost :online="online" />
            <RouterView />
          </div>
        </main>
        <AppNavigation v-if="layout.navigation === 'bottom'" bottom :rail="false" @search="searchPalette?.show($event)" />
      </div>
    </div>
  </ConfigProvider>
</template>

<script setup lang="ts">
import type { DevicesSnapshot } from '@synra/protocol'
import { synraAntdTheme } from '@synra/theme/antd'
import { message as antdMessage, ConfigProvider } from 'ant-design-vue'
import enUS from 'ant-design-vue/es/locale/en_US'
import zhCN from 'ant-design-vue/es/locale/zh_CN'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { hostEnvironment } from './app/host-environment'
import { kernel } from './app/kernel'
import { locale, watchSystemLocale } from './app/locale'
import { useNetworkStatus } from './app/network'
import { useHostBack } from './app/use-host-back'
import { useViewportLayout } from './app/viewport'
import AppNavigation from './components/AppNavigation.vue'
import CrashNotice from './components/CrashNotice.vue'
import DebugConsole from './components/DebugConsole.vue'
import InputPrompt from './components/InputPrompt.vue'
import NoticeHost from './components/NoticeHost.vue'
import PairingPrompt from './components/PairingPrompt.vue'
import SearchPalette from './components/SearchPalette.vue'
import TransferPrompt from './components/TransferPrompt.vue'
import WindowTitleBar from './components/WindowTitleBar.vue'
import { messages } from './messages'
import { useDevicesStore } from './stores/devices'
import { useSettingsStore } from './stores/settings'

const route = useRoute()
const router = useRouter()
const devices = useDevicesStore()
const settings = useSettingsStore()
const componentTheme = synraAntdTheme()
const componentLocale = computed(() => locale.value === 'zh-CN'
  ? zhCN
  : enUS)
const online = useNetworkStatus()
const contentPane = ref<HTMLElement>()
const searchPalette = ref<{ show: (trigger?: HTMLElement) => void }>()
const environment = computed(() => hostEnvironment(window, devices.identity?.platform))
const layout = useViewportLayout()
let stopStartup: (() => void) | undefined

watch(locale, value => document.documentElement.lang = value, { immediate: true })
watch(() => route.path, () => contentPane.value?.scrollTo({ top: 0, left: 0 }), { flush: 'post' })
useHostBack(environment, router, () => {
  void antdMessage.info(messages.app.backAgainToExit)
})

onMounted(() => {
  let active = true
  const stopLocaleSync = watchSystemLocale()
  const preventContextMenu = (event: MouseEvent): void => event.preventDefault()
  document.addEventListener('contextmenu', preventContextMenu)
  stopStartup = () => {
    active = false
    stopLocaleSync()
    document.removeEventListener('contextmenu', preventContextMenu)
  }

  void kernel.call<DevicesSnapshot>('devices_snapshot').then((answer) => {
    if (active && answer.ok)
      devices.applySnapshot(answer.value)
  }).catch((error: unknown) => console.warn('Synra device snapshot', error))
  void settings.read(kernel).catch((error: unknown) => console.warn('Synra settings', error))
})
onUnmounted(() => stopStartup?.())
</script>
