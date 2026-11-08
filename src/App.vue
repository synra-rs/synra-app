<style>
:global(html),
:global(body),
:global(#app) {
  width: 100%;
  height: 100%;
  overflow: hidden;
  overscroll-behavior: none;
  touch-action: pan-x pan-y;
  -ms-touch-action: pan-x pan-y;
  -webkit-text-size-adjust: 100%;
  -webkit-user-select: none;
  user-select: none;
}

:global(input),
:global(textarea),
:global([contenteditable="true"]) {
  -webkit-user-select: text;
  user-select: text;
}

:global(body),
:global(button),
:global(a),
:global(summary),
:global([role="button"]) {
  cursor: default;
}

:global(input),
:global(textarea),
:global(select),
:global(button),
:global(a),
:global(summary),
:global([role="button"]) {
  cursor: default;
}

.screen {
  position: fixed;
  inset: 0;
  display: grid;
  min-height: 0;
  min-width: 0;
  overflow: hidden;
  background-image: radial-gradient(58rem 40rem at 24% -32%, color-mix(in oklab, var(--synra-primary) 10%, transparent), transparent);
  background-attachment: fixed;
}

.desktop-shell {
  min-height: 0;
  min-width: 0;
}

.desktop-main {
  min-height: 0;
  min-width: 0;
}

.screen.mobile-shell {
  display: flex;
  flex-direction: column;
  height: 100dvh;
}

.mobile-shell > .desktop-shell {
  display: flex;
  flex: 1 1 auto;
  flex-direction: column;
  min-height: 0;
  height: auto;
}

.mobile-shell > .desktop-shell > :first-child {
  display: none;
}

.mobile-shell > .desktop-shell > main {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  height: auto;
  padding: 1.25rem;
  padding-bottom: 1.5rem;
  font-size: 1.125rem;
}

.mobile-shell > .desktop-shell > nav {
  position: relative;
  flex: 0 0 auto;
  display: flex;
  box-sizing: border-box;
  min-height: calc(4.25rem + env(safe-area-inset-bottom));
  padding: 0.35rem max(0.5rem, env(safe-area-inset-left)) env(safe-area-inset-bottom) max(0.5rem, env(safe-area-inset-right));
  gap: 0.25rem;
  border-top: 1px solid color-mix(in oklab, var(--synra-line) 70%, transparent);
  background-color: var(--synra-glass);
  -webkit-backdrop-filter: blur(var(--synra-blur)) saturate(var(--synra-saturate));
  backdrop-filter: blur(var(--synra-blur)) saturate(var(--synra-saturate));
}

.mobile-shell > .desktop-shell > nav button {
  min-height: 3.5rem;
  font-size: 0.875rem;
}

.mobile-shell {
  padding-top: env(safe-area-inset-top);
}

.mobile-shell .mobile-tabbar button {
  appearance: none;
  -webkit-appearance: none;
  min-width: 0;
  margin: 0;
  padding: 0.35rem 0.25rem;
  border: 0;
  border-radius: var(--synra-radius-control);
  background: transparent;
  color: var(--synra-ink-dim);
  font: inherit;
  line-height: 1.2;
  text-align: center;
  cursor: pointer;
  transition: color var(--synra-duration-fast), background-color var(--synra-duration-fast);
}

.mobile-shell .mobile-tabbar button[aria-current="page"] {
  color: var(--synra-primary-ink);
  background-color: color-mix(in oklab, var(--synra-primary) 14%, transparent);
}

.mobile-shell > .desktop-shell > main > div {
  max-width: none;
}

.mobile-shell :deep(.ant-collapse-header),
.mobile-shell :deep(.ant-collapse-content-box) {
  font-size: 1.125rem;
}

.mobile-shell :deep(.ant-collapse-header) {
  min-height: 3.5rem;
  padding-block: 1rem;
}

.mobile-shell :deep(.ant-btn),
.mobile-shell :deep(.ant-input),
.mobile-shell :deep(.ant-select-selector) {
  min-height: 3rem;
  font-size: 1.0625rem;
}

.mobile-shell :deep(.ant-list-item) {
  padding-block: 0.9rem;
}

@supports not (backdrop-filter: blur(0)) {
  .mobile-shell > .desktop-shell > nav {
    background-color: var(--synra-glass-base);
  }
}
</style>

<template>
  <ConfigProvider :theme="componentTheme" :locale="componentLocale" :auto-insert-space-in-button="false">
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
import AppNavigation from './components/AppNavigation.vue'
import CrashNotice from './components/CrashNotice.vue'
import DebugConsole from './components/DebugConsole.vue'
import InputPrompt from './components/InputPrompt.vue'
import NoticeHost from './components/NoticeHost.vue'
import PairingPrompt from './components/PairingPrompt.vue'
import SearchPalette from './components/SearchPalette.vue'
import TransferPrompt from './components/TransferPrompt.vue'
import WindowTitleBar from './components/WindowTitleBar.vue'
import { kernel } from './kernel'
import { locale, watchSystemLocale } from './locale'
import { messages } from './messages'
import { useNetworkStatus } from './network'
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
