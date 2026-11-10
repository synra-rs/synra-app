<style src="./app/shell.css"></style>

<template>
  <ConfigProvider :theme="componentTheme" :locale="componentLocale" :auto-insert-space-in-button="false" :wave="{ disabled: false }">
    <TransferPrompt />
    <PairingPrompt />
    <InputPrompt />
    <DebugConsole />
    <SearchPalette ref="searchPalette" />
    <div class="screen grid-rows-[auto_minmax(0,1fr)]" :class="{ 'mobile-shell': isMobile }">
      <WindowTitleBar />
      <div class="desktop-shell grid min-h-0 min-w-0 grid-rows-[minmax(0,1fr)_auto] md:grid-cols-[15rem_minmax(0,1fr)]">
        <AppNavigation :rail="rail" @search="searchPalette?.show()" />
        <main ref="contentPane" tabindex="0" :aria-label="messages.app.content" class="desktop-main [scrollbar-gutter:stable] min-h-0 min-w-0 overflow-auto overscroll-contain px-4 py-5 text-base focusable lg:px-10 md:px-8 sm:px-6 focus-visible:ring-inset">
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
    </div>
  </ConfigProvider>
</template>

<script setup lang="ts">
import type { DevicesSnapshot } from '@synra/protocol'
import { synraAntdTheme } from '@synra/theme/antd'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { message as antdMessage, ConfigProvider } from 'ant-design-vue'
import enUS from 'ant-design-vue/es/locale/en_US'
import zhCN from 'ant-design-vue/es/locale/zh_CN'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { kernel } from './app/kernel'
import { locale, watchSystemLocale } from './app/locale'
import { useNetworkStatus } from './app/network'
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
const componentLocale = computed(() => locale.value === 'zh-CN' ? zhCN : enUS)
const online = useNetworkStatus()
const contentPane = ref<HTMLElement>()
const searchPalette = ref<{ show: () => void }>()
const rail = ref(false)
const isMobile = ref(false)
const isTauri = '__TAURI_INTERNALS__' in window
let query: MediaQueryList | undefined
let removeContextMenu: (() => void) | undefined
let removeGestureListeners: (() => void) | undefined
let backResetTimer: ReturnType<typeof setTimeout> | undefined
let lastBackAt = 0
let stopBack: (() => void) | undefined
let stopLocaleSync = (): void => {}

watch(locale, value => document.documentElement.lang = value, { immediate: true })
watch(() => route.path, () => contentPane.value?.scrollTo({ top: 0, left: 0 }), { flush: 'post' })

function apply(event: MediaQueryList | MediaQueryListEvent): void {
  rail.value = !event.matches
}

function isPrimaryRoute(path: string): boolean {
  return path === '/' || path === '/devices' || path === '/plugins' || path === '/settings'
}

onMounted(async () => {
  stopLocaleSync = watchSystemLocale()
  query = window.matchMedia('(min-width: 64rem)')
  const phoneLike = /android|iphone|ipad|ipod|mobile/i.test(navigator.userAgent) || navigator.maxTouchPoints > 0
  isMobile.value = isTauri && phoneLike ? true : !query.matches
  rail.value = !isMobile.value && !query.matches
  query.addEventListener('change', (event) => {
    if (!isTauri || !phoneLike) {
      apply(event)
      isMobile.value = !event.matches
    }
  })
  const preventContextMenu = (event: MouseEvent): void => event.preventDefault()
  document.addEventListener('contextmenu', preventContextMenu)
  removeContextMenu = () => document.removeEventListener('contextmenu', preventContextMenu)
  const preventZoom = (event: TouchEvent): void => {
    if (event.touches.length > 1)
      event.preventDefault()
  }
  document.addEventListener('touchstart', preventZoom, { passive: false })
  document.addEventListener('touchmove', preventZoom, { passive: false })
  removeGestureListeners = () => {
    document.removeEventListener('touchstart', preventZoom)
    document.removeEventListener('touchmove', preventZoom)
  }
  if ('ontouchstart' in window) {
    const preventGesture = (event: Event): void => event.preventDefault()
    for (const type of ['gesturestart', 'gesturechange', 'gestureend'])
      document.addEventListener(type, preventGesture, { passive: false })
    const removeGestureEvents = removeGestureListeners
    removeGestureListeners = () => {
      removeGestureEvents?.()
      for (const type of ['gesturestart', 'gesturechange', 'gestureend'])
        document.removeEventListener(type, preventGesture)
    }
  }
  if (isMobile.value && isTauri) {
    try {
      const windowHandle = getCurrentWindow()
      await windowHandle.setFullscreen(true)
    }
    catch {
      // Mobile builds may not expose the desktop window API; safe-area CSS remains the fallback.
    }
  }
  if (isMobile.value && isTauri) {
    const windowHandle = getCurrentWindow()
    let allowClose = false
    stopBack = await windowHandle.onCloseRequested(async (event) => {
      if (allowClose) {
        allowClose = false
        return
      }
      event.preventDefault()
      if (!isPrimaryRoute(route.path)) {
        await router.back()
        return
      }
      const now = Date.now()
      if (now - lastBackAt < 1800) {
        allowClose = true
        await windowHandle.close()
        return
      }
      lastBackAt = now
      antdMessage.info(messages.app.backAgainToExit)
      if (backResetTimer)
        clearTimeout(backResetTimer)
      backResetTimer = setTimeout(() => {
        lastBackAt = 0
      }, 1800)
    })
  }
  const answer = await kernel.call<DevicesSnapshot>('devices_snapshot')
  if (answer.ok)
    devices.applySnapshot(answer.value)
  await settings.read(kernel)
})

onUnmounted(() => {
  stopLocaleSync()
  stopBack?.()
  removeContextMenu?.()
  removeGestureListeners?.()
  if (backResetTimer)
    clearTimeout(backResetTimer)
  query?.removeEventListener('change', apply)
})
</script>
