<template>
  <section class="stack gap-6">
    <h1 class="text-xl font-semibold">
      {{ messages.plugins.title }}
    </h1>

    <ErrorNotice v-if="error" :code="error.code" :message="error.message" :actions="error.actions" @retry="load" />

    <!--
      R28: an empty state is an invitation with a next step. This screen cannot install anything yet, so the invitation
      is the one thing it *can* do truthfully — read the list again — and the button reports that it is doing it.
    -->
    <AEmpty v-if="plugins?.length === 0" :description="messages.plugins.empty">
      <template #image>
        <!--
          The icon rather than antd's default illustration: that illustration draws its own light-grey palette into a
          screen whose neutrals are all one hue (§1.4), and empty states are named as the places that must not compete
          for attention (R27).
        -->
        <span class="i-carbon-application text-xl text-ink-dim" aria-hidden="true" />
      </template>

      <AButton :loading="loading" @click="load">
        {{ messages.plugins.emptyAction }}
      </AButton>
    </AEmpty>

    <!--
      A plugin is a substantial object — a name, three machine facts, a description, two permission lists and possibly
      an update — so each one is a list item with several lines rather than a card (R26). The lines are labelled
      separately instead of joined by separators, which is the shape R21 retires.
    -->
    <AList v-else :data-source="installed">
      <template #renderItem="{ item: plugin }">
        <AList.Item>
          <div class="w-full stack gap-3">
            <div class="row-between gap-4">
              <span class="truncate text-base font-medium">{{ plugin.title }}</span>

              <!-- R23: the state is the word in the tag; the colour is the redundancy that survives a greyscale shot. -->
              <ATag :color="stateColour(plugin.state)" class="shrink-0">
                {{ plugin.state }}
              </ATag>
            </div>

            <dl class="m-0 row flex-wrap gap-4 text-xs">
              <div class="row gap-1">
                <dt class="m-0 text-ink-dim">
                  {{ messages.plugins.pluginId }}
                </dt>
                <dd class="m-0 font-mono">
                  {{ plugin.pluginId }}
                </dd>
              </div>
              <div class="row gap-1">
                <dt class="m-0 text-ink-dim">
                  {{ messages.plugins.version }}
                </dt>
                <dd class="m-0 font-mono">
                  {{ plugin.version }}
                </dd>
              </div>
              <div class="row gap-1">
                <dt class="m-0 text-ink-dim">
                  {{ messages.plugins.publisher }}
                </dt>
                <dd class="m-0 font-mono">
                  {{ plugin.publisher }}
                </dd>
              </div>
            </dl>

            <p v-if="plugin.description" class="m-0 text-sm text-ink-dim">
              {{ plugin.description }}
            </p>

            <!--
              What the plugin asked for, and what it was refused: the second list is the one that turns this screen from
              a settings page into the thing a user reads before installing something. "Nothing" is a word rather than
              an empty space, because an empty space reads as "not loaded".
            -->
            <div class="stack gap-1">
              <div class="row flex-wrap gap-2">
                <span class="text-xs text-ink-dim">{{ messages.plugins.granted }}</span>
                <span v-if="plugin.granted?.length === 0" class="text-xs text-ink-dim">{{ messages.plugins.none }}</span>
                <ATag v-for="capability in plugin.granted" :key="capability" class="font-mono">
                  {{ capability }}
                </ATag>
              </div>

              <div class="row flex-wrap gap-2">
                <span class="text-xs text-ink-dim">{{ messages.plugins.refused }}</span>
                <span v-if="plugin.denied?.length === 0" class="text-xs text-ink-dim">{{ messages.plugins.none }}</span>
                <ATag v-for="capability in plugin.denied" :key="capability" color="error" class="font-mono">
                  {{ capability }}
                </ATag>
              </div>
            </div>

            <!--
              An update is a decision rather than routine news when it asks for something new, so it is an alert with
              the version and the added permissions in it — and the permissions are named, not counted (R33's spirit:
              the user is told what they are being asked to allow).
            -->
            <AAlert v-if="plugin.update" type="warning" show-icon :message="messages.plugins.updateAvailable">
              <template #description>
                <div class="stack gap-1">
                  <span class="row gap-1 text-sm">
                    <span class="text-ink-dim">{{ messages.plugins.version }}</span>
                    <span class="font-mono">{{ plugin.update.version }}</span>
                  </span>

                  <template v-if="plugin.update.needsNewConsent">
                    <span class="text-sm">{{ messages.plugins.updateNeedsConsent }}</span>
                    <span class="row flex-wrap gap-1">
                      <ATag v-for="capability in plugin.update.addedCapabilities" :key="capability" class="font-mono">
                        {{ capability }}
                      </ATag>
                    </span>
                  </template>
                </div>
              </template>
            </AAlert>
          </div>
        </AList.Item>
      </template>
    </AList>
  </section>
</template>

<script setup lang="ts">
/**
 * What is installed, and what each one has been allowed to do.
 *
 * # Why granted and denied are shown rather than only granted
 *
 * A permissions screen that lists what a plugin *may* do tells the user nothing about what it *has tried* to do. The
 * design document asks for both, and it is the denied column that turns the screen from a settings page into the thing a
 * user actually reads before installing something.
 *
 * # Why the lifecycle state is a field and not a colour
 *
 * The second plugin in the fake snapshot is `enabled` **and** `faulted`. A screen that derived its badge from `enabled`
 * would show that plugin as working, which is the specific mistake `PluginSnapshot.state` exists to prevent.
 */
import type { PluginSnapshot, SuggestedAction } from '@synra/protocol'
import { Alert as AAlert, Button as AButton, Empty as AEmpty, List as AList, Tag as ATag } from 'ant-design-vue'
import { computed, onMounted, ref } from 'vue'

import ErrorNotice from '../components/ErrorNotice.vue'
import { kernel } from '../kernel'
import { messages } from '../messages'

const plugins = ref<readonly PluginSnapshot[]>([])

/**
 * The plugins as the list takes them.
 *
 * Copied because `AList`'s `dataSource` is typed as a mutable array and what the kernel answered is `readonly`. The
 * copy is made when the list changes rather than on every render.
 */
const installed = computed(() => [...plugins.value])

/** The refusal itself, not its sentence: the surface needs the `code`, the message **key** and the `actions` (R33). */
const error = ref<{ code: string, message: string, actions?: readonly SuggestedAction[] | undefined } | undefined>(undefined)

/** Whether the list is being read, so the empty state's button can say that it is doing something. */
const loading = ref(false)

/**
 * The lifecycle state's tag colour.
 *
 * `enabled` is deliberately not consulted: a plugin that is enabled **and** faulted is exactly what
 * `PluginSnapshot.state` exists to report. The two states this milestone knows get a status colour and anything the
 * kernel invents later stays neutral, which is the honest default — the tag always carries the word (R23), so the
 * colour is redundancy rather than the state itself.
 */
function stateColour(state: string): 'success' | 'error' | 'default' {
  switch (state) {
    case 'active': return 'success'
    case 'faulted': return 'error'
    default: return 'default'
  }
}

onMounted(load)

/** Reads the installed plugins. Startup and the empty state's button are the same read. */
async function load(): Promise<void> {
  loading.value = true
  error.value = undefined

  const answer = await kernel.call<readonly PluginSnapshot[]>('plugins_list')
  loading.value = false

  if (answer.ok)
    plugins.value = answer.value
  else
    error.value = { code: answer.error.code, message: answer.error.message, actions: answer.error.actions }
}
</script>
