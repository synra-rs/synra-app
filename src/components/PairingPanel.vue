<template>
  <!--
    ★ A dialog on a wide screen, a drawer on a narrow one.

      Not a card in the page: pairing is something a person starts and finishes, and the device list underneath is not
      what they are looking at while it runs. A card scrolled into view also pushed the list down the moment it opened,
      which moved the row the person had just pressed.

      The drawer on a narrow screen for the same reason the modal is not: the surface is a full width already, so a
      centred box with margins either side would be narrower than the page it sits on and lose the room a long device id
      needs. A drawer takes the full width and slides in from where a thumb already is.
  -->
  <AModal
    v-if="visible && !narrow"
    :open="true"
    :title="messages.devices.pairingTitle"
    :footer="null"
    :get-container="false"
    :width="480"
    @cancel="emit('close')"
  >
    <div class="pairing-panel">
      <PanelBody />
    </div>
  </AModal>
  <ADrawer
    v-else-if="visible"
    :open="true"
    :title="messages.devices.pairingTitle"
    placement="bottom"
    :get-container="false"
    @close="emit('close')"
  >
    <div class="pairing-panel">
      <PanelBody />
    </div>
  </ADrawer>
</template>

<script setup lang="ts">
import type { DeviceCard } from '@synra/protocol'
import { Drawer as ADrawer, Modal as AModal } from 'ant-design-vue'

import { useNarrowLayout } from '../composables/use-narrow-layout'
import { messages } from '../messages'
import PanelBody from './PairingPanelBody.vue'

withDefaults(defineProps<{ selected?: DeviceCard | undefined, visible: boolean }>(), {
  selected: undefined
})
const emit = defineEmits<{ close: [] }>()

const narrow = useNarrowLayout()
</script>
