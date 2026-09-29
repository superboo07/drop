<template>
  <span
    :class="[
      clientReleaseStatusStyles[props.status].class,
      'inline-flex items-center rounded-md px-2 py-1 text-xs font-medium ring-1 ring-inset',
    ]"
  >
    {{ clientReleaseStatusStyles[props.status].label }}
  </span>
</template>

<script setup lang="ts">
const props = defineProps<{ status: ClientReleaseStatus }>();
</script>
