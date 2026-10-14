<template>
  <span
    :class="[
      clientLogTriggerStyles[props.trigger].class,
      'inline-flex items-center whitespace-nowrap rounded-md px-2 py-1 text-xs font-medium ring-1 ring-inset',
    ]"
  >
    {{ labels[props.trigger] }}
  </span>
</template>

<script setup lang="ts">
import {
  clientLogTriggerStyles,
  type ClientLogTriggerSlug,
} from "~/utils/clientLogs";

const props = defineProps<{ trigger: ClientLogTriggerSlug }>();
const labels = useClientLogTriggerLabels();
</script>
