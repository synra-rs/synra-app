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
      <div class="row items-center gap-2 border border-line rounded-control bg-raised px-3">
        <span class="i-carbon-search shrink-0 text-ink-dim" aria-hidden="true" />
        <AInput
          :ref="setField"
          v-model:value="query"
          type="text"
          class="min-w-0 flex-1" :bordered="false"
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
      <p v-if="!query.trim()" class="m-0 text-sm text-ink-dim">
        {{ messages.search.scope }}
      </p>
      <div v-if="results.length > 0" id="search-results" class="max-h-80 overflow-y-auto overscroll-contain" role="listbox" :aria-label="messages.search.title">
        <template v-for="group in grouped" :key="group.id">
          <div class="mb-1 mt-1 text-sm text-ink-dim">
            {{ group.label }}
          </div>
          <button
            v-for="item in group.items"
            :id="optionDomId(item.id)"
            :key="item.id"
            type="button"
            role="option"
            :aria-selected="item.id === activeId"
            class="synra-choice min-h-8 w-full row items-center gap-2 border-0 rounded-control bg-transparent px-2 text-start text-base hover:bg-surface"
            :class="{ 'bg-raised': item.id === activeId }"
            @click="choose(item)"
            @mouseenter="activeId = item.id"
          >
            <span :class="item.icon" class="shrink-0 text-ink-dim" aria-hidden="true" />
            <span class="min-w-0 flex-1 truncate">{{ item.label }}</span>
            <span v-if="item.detail" class="shrink-0 text-sm text-ink-dim">{{ item.detail }}</span>
          </button>
        </template>
      </div>
      <p v-else-if="query.trim() !== ''" class="m-0 text-base text-ink-dim">
        {{ messages.search.empty }}
      </p>
      <p v-else class="m-0 text-base text-ink-dim">
        {{ messages.search.scope }}
      </p>
    </div>
  </AModal>
</template>

<script setup lang="ts">
import { Button as AButton, Input as AInput, Modal as AModal } from 'ant-design-vue'
import { messages } from '../messages'
import { useDialogVisibility } from '../stores/dialogs'
import { useSearchPalette } from './search/use-search-palette'

const { open, query, setField, activeId, results, grouped, optionDomId, show, close, choose, onKeydown } = useSearchPalette()
const visible = useDialogVisibility(open, 90)
defineExpose({ show })
</script>
