<template>
  <header class="px-3 border-b border-line bg-glass flex shrink-0 h-11 min-h-11 items-center" data-tauri-drag-region @dblclick="toggleMaximize">
    <div class="row flex-1 gap-2 min-w-0" data-tauri-drag-region>
      <span class="i-carbon-connect text-lg text-primary" aria-hidden="true" />
      <span class="text-sm font-semibold truncate">{{ messages.app.name }}</span>
    </div>
    <div class="row gap-1" @mousedown.stop @dblclick.stop>
      <AButton ghost type="default" size="small" :aria-label="messages.window.minimize" @click="minimize">
        <span class="i-carbon-minimize" aria-hidden="true" />
      </AButton>
      <AButton ghost type="default" size="small" :aria-label="messages.window.maximize" @click="toggleMaximize">
        <span :class="maximized ? 'i-carbon-window-auto' : 'i-carbon-maximize'" aria-hidden="true" />
      </AButton>
      <AButton ghost type="default" size="small" danger :aria-label="messages.window.close" @click="close">
        <span class="i-carbon-close" aria-hidden="true" />
      </AButton>
    </div>
  </header>
</template>

<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { Button as AButton } from 'ant-design-vue'
import { onMounted, onUnmounted, ref } from 'vue'
import { subscribeWhileActive } from '../app/native-listener'
import { messages } from '../messages'

const windowHandle = getCurrentWindow()
const maximized = ref(false)
let unlistenResize: (() => void) | undefined
const report = (error: unknown): void => console.warn('Synra window size', error)

async function minimize(): Promise<void> {
  await windowHandle.minimize()
}
async function close(): Promise<void> {
  await windowHandle.close()
}
async function toggleMaximize(): Promise<void> {
  if (await windowHandle.isMaximized()) {
    await windowHandle.unmaximize()
  }
  else {
    await windowHandle.maximize()
  }
  maximized.value = await windowHandle.isMaximized()
}

onMounted(() => {
  unlistenResize = subscribeWhileActive(async (isActive) => {
    async function refresh(): Promise<void> {
      const next = await windowHandle.isMaximized()
      if (isActive())
        maximized.value = next
    }
    await refresh().catch(report)
    return windowHandle.onResized(() => {
      if (isActive())
        void refresh().catch(report)
    })
  }, report)
})
onUnmounted(() => unlistenResize?.())
</script>
