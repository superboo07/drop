<template>
  <span
    :class="[
      clientReleaseBranchStyles[props.branch].class,
      'inline-flex items-center rounded-md px-2 py-1 text-xs font-medium ring-1 ring-inset',
    ]"
  >
    {{ clientReleaseBranchStyles[props.branch].label }}
  </span>
</template>

<script setup lang="ts">
const props = defineProps<{ branch: ClientReleaseBranchSlug }>();
</script>
