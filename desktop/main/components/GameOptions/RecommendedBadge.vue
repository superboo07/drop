<template>
  <span
    v-if="state?.state === 'recommended'"
    class="ml-2 inline-flex items-center rounded-full bg-blue-600/15 px-2 py-0.5 align-middle text-xs font-medium text-blue-400"
    >Recommended</span
  >
  <span
    v-else-if="state?.state === 'changed'"
    class="ml-2 inline-flex items-center rounded-full bg-amber-400/10 px-2 py-0.5 align-middle text-xs font-medium text-amber-400"
    >Changed by you · server: {{ state.server }}</span
  >
</template>

<script setup lang="ts">
import type { RecommendationState } from "~/composables/proton-defaults";

defineProps<{ state: RecommendationState }>();
</script>
