<template>
  <header class="hidden h-11 min-h-11 shrink-0 items-center border-b border-line bg-glass px-3 md:flex" data-tauri-drag-region @dblclick="toggleMaximize">
    <div class="row min-w-0 flex-1 gap-2" data-tauri-drag-region>
      <span class="i-carbon-connect text-lg text-primary" aria-hidden="true" />
      <span class="truncate text-sm font-semibold">{{ messages.app.name }}</span>
    </div>
    <div class="row gap-1" @mousedown.stop @dblclick.stop>
      <AButton type="ghost" size="small" :aria-label="messages.window.minimize" @click="minimize"><span class="i-carbon-subtract" aria-hidden="true" /></AButton>
      <AButton type="ghost" size="small" :aria-label="messages.window.maximize" @click="toggleMaximize"><span :class="maximized ? 'i-carbon-collapse-all' : 'i-carbon-maximize'" aria-hidden="true" /></AButton>
      <AButton type="ghost" size="small" danger :aria-label="messages.window.close" @click="close"><span class="i-carbon-close" aria-hidden="true" /></AButton>
    </div>
  </header>
</template>

<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { Button as AButton } from 'ant-design-vue'
import { onMounted, onUnmounted, ref } from 'vue'
import { messages } from '../messages'

const isTauri = '__TAURI_INTERNALS__' in window
const windowHandle = isTauri ? getCurrentWindow() : undefined
const maximized = ref(false)
let unlistenResize: (() => void) | undefined

async function minimize(): Promise<void> {
  await windowHandle?.minimize()
}
async function close(): Promise<void> {
  await windowHandle?.close()
}
async function toggleMaximize(): Promise<void> {
  if (!windowHandle)
    return
  if (await windowHandle.isMaximized()) {
    await windowHandle.unmaximize()
  }
  else {
    await windowHandle.maximize()
  }
  maximized.value = await windowHandle.isMaximized()
}

onMounted(async () => {
  if (!windowHandle)
    return
  maximized.value = await windowHandle.isMaximized()
  unlistenResize = await windowHandle.onResized(async () => {
    maximized.value = await windowHandle.isMaximized()
  })
})
onUnmounted(() => unlistenResize?.())
</script>
