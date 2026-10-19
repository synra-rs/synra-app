<template>
  <!--
    ★ The kernel's page-less notice, at the root beside the crash banner. A received file is news for whoever is looking at
    the screen, not for one page: the progress row disappears when the transfer finishes, and this is what replaces it.
  -->
  <AAlert
    v-if="notice"
    :type="alertType"
    show-icon
    closable
    :message="headline"
    :description="detail"
    class="mb-4"
    @close="notices.dismiss()"
  />
</template>

<script setup lang="ts">
/**
 * Draws the latest `notice` event.
 *
 * # Why the message is a key and not a sentence
 *
 * The kernel sends `message` plus `params`, not English. The sentence is this side's, keyed by that word — the same rule the
 * error surface follows, and the reason a notice can be translated without touching the kernel.
 *
 * # Why an unknown key still shows something
 *
 * Because a kernel newer than the window may send a notice this build has never heard of. Printing the key would put an
 * engine word on screen (R34); showing nothing would make the event look lost. A generic sentence is the honest middle.
 */
import { Alert as AAlert } from 'ant-design-vue'
import { computed } from 'vue'

import { messages } from '../messages'
import { useNoticesStore } from '../stores/notices'

const notices = useNoticesStore()
const notice = computed(() => notices.latest)

/** A link this build does not understand is a warning rather than a success. */
const alertType = computed(() => notice.value?.message === 'notice.unknownLink' ? 'warning' : 'success')

const headline = computed(() => {
  const value = notice.value
  if (!value)
    return ''
  if (value.message === 'notice.received')
    return messages.notices.received(String(value.params.name ?? ''))
  if (value.message === 'notice.unknownLink')
    return messages.notices.unknownLink(String(value.params.url ?? ''))
  return messages.notices.generic
})

const detail = computed(() => {
  const value = notice.value
  if (!value || value.message !== 'notice.received')
    return undefined
  const path = value.params.path
  return typeof path === 'string' ? messages.notices.savedTo(path) : undefined
})
</script>
