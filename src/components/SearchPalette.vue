<template>
  <AModal
    :open="visible"
    :footer="null"
    :closable="false"
    :width="560"
    :destroy-on-close="false"
    :wrap-props="{ 'aria-label': messages.search.title }"
    wrap-class-name="search-modal"
    @cancel="close"
  >
    <div class="stack gap-3">
      <div class="px-3 border border-line rounded-control bg-raised row gap-2 items-center">
        <span :class="icons.search" class="text-ink-dim shrink-0" aria-hidden="true" />
        <AInput
          :ref="setField"
          v-model:value="query"
          type="text"
          class="flex-1 min-w-0" :bordered="false"
          :placeholder="messages.search.placeholder"
          :aria-label="messages.search.placeholder"
          role="combobox"
          aria-controls="search-results"
          :aria-expanded="results.length > 0"
          :aria-activedescendant="activeId ? optionDomId(activeId) : undefined"
          @keydown="onKeydown"
        />
        <AButton type="text" class="synra-icon-button" :aria-label="messages.search.close" @click="close">
          <span class="i-carbon-close" aria-hidden="true" />
        </AButton>
      </div>
      <p v-if="!query.trim()" class="text-sm text-ink-dim m-0">
        {{ messages.search.scope }}
      </p>
      <div v-if="results.length > 0" id="search-results" class="overscroll-contain max-h-80 overflow-y-auto" role="listbox" :aria-label="messages.search.title">
        <template v-for="group in grouped" :key="group.id">
          <div class="text-sm text-ink-dim mb-1 mt-1">
            {{ group.label }}
          </div>
          <button
            v-for="item in group.items"
            :id="optionDomId(item.id)"
            :key="item.id"
            type="button"
            role="option"
            :aria-selected="item.id === activeId"
            class="synra-choice text-base px-2 text-start border-0 rounded-control bg-transparent row gap-2 min-h-8 w-full items-center hover:bg-surface"
            :class="{ 'bg-raised': item.id === activeId }"
            @click="choose(item)"
            @mouseenter="activeId = item.id"
          >
            <span :class="item.icon" class="text-ink-dim shrink-0" aria-hidden="true" />
            <span class="flex-1 min-w-0 truncate">{{ item.label }}</span>
            <span v-if="item.detail" class="text-sm text-ink-dim shrink-0">{{ item.detail }}</span>
          </button>
        </template>
      </div>
      <p v-else-if="query.trim() !== ''" class="text-base text-ink-dim m-0">
        {{ messages.search.empty }}
      </p>
      <p v-else class="text-base text-ink-dim m-0">
        {{ messages.search.scope }}
      </p>
    </div>
  </AModal>
</template>

<script setup lang="ts">
import { Button as AButton, Input as AInput, Modal as AModal } from 'ant-design-vue'
import { icons } from '../app/icon-map'
import { messages } from '../messages'
import { useDialogVisibility } from '../stores/dialogs'
import { useSearchPalette } from './search/use-search-palette'

const { open, query, setField, activeId, results, grouped, optionDomId, show, close, choose, onKeydown } = useSearchPalette()
const visible = useDialogVisibility(open, 90, close)
defineExpose({
  show,
  close,
  isVisible: () => visible.value
})
</script>
