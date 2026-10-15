/**
 * The shell.
 *
 * Three lines of mount code and nothing else, because every decision about what the interface *does* belongs in the
 * pages or in the stores. A `main.ts` that grows is the same failure as a `main.rs` that grows, and the kernel side
 * guards that with a line count for exactly the same reason.
 */

import { createPinia } from 'pinia'
import { createApp } from 'vue'

import App from './App.vue'
import { router } from './router'

createApp(App).use(createPinia()).use(router).mount('#app')
