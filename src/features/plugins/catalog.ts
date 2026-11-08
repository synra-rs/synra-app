import type { CatalogItem, CatalogSort } from '@synra/protocol'
import type { KernelClient } from '../../ipc/client'
import type { Failure } from './actions'
import { computed, onScopeDispose, ref, watch } from 'vue'
import { searchCatalog } from './actions'

const PAGE_SIZE = 20

/** A submitted registry query owns its pages. Draft changes invalidate reads without starting background requests. */
export function useCatalog(client: KernelClient) {
  const text = ref('')
  const sort = ref<CatalogSort>('relevance')
  const results = ref<CatalogItem[]>([])
  const searched = ref(false)
  const error = ref<Failure>()
  const busy = ref<'search' | 'more'>()
  const hasMore = ref(false)
  let offset = 0
  let revision = 0
  let alive = true
  let lastReadMore = false

  watch([text, sort], () => {
    revision++
    results.value = []
    searched.value = false
    error.value = undefined
    hasMore.value = false
    offset = 0
  }, { flush: 'sync' })

  onScopeDispose(() => {
    alive = false
    revision++
  })

  async function read(more: boolean): Promise<void> {
    const query = text.value.trim()
    // One outstanding read per page; editing a draft does not create an unbounded tail of IPC calls.
    if (!alive || !query || busy.value || (more && !hasMore.value))
      return
    const current = revision
    const start = more ? offset : 0
    lastReadMore = more
    busy.value = more ? 'more' : 'search'
    error.value = undefined
    const outcome = await searchCatalog(client, query, { offset: start, sort: sort.value })
    if (!alive)
      return
    busy.value = undefined
    if (current !== revision)
      return
    if (!outcome.ok) {
      error.value = outcome.failure
      return
    }
    const known = new Set(more ? results.value.map(item => item.pluginId) : [])
    const incoming = outcome.value.filter((item) => {
      if (known.has(item.pluginId))
        return false
      known.add(item.pluginId)
      return true
    })
    results.value = more ? [...results.value, ...incoming] : incoming
    // Server offsets count rows, including duplicates; displayed row count is not a valid continuation cursor.
    offset = start + outcome.value.length
    hasMore.value = outcome.value.length === PAGE_SIZE
    searched.value = true
  }

  return {
    text,
    sort,
    results,
    searched,
    error,
    hasMore,
    searching: computed(() => busy.value === 'search'),
    loadingMore: computed(() => busy.value === 'more'),
    busy: computed(() => busy.value !== undefined),
    search: () => read(false),
    loadMore: () => read(true),
    retry: () => read(lastReadMore)
  }
}
