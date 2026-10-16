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
// ★ `uno.css` is the stylesheet `@synra/theme` produces: the tokens as custom properties, the reset, and the utilities
// the templates actually use. It is imported **explicitly** rather than relying on the plugin to inject it, so that the
// one line that brings in every colour and every utility is visible in the entry point — an injected stylesheet is a
// stylesheet nobody can find when it is missing.
import 'uno.css'

createApp(App).use(createPinia()).use(router).mount('#app')
