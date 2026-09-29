<template />

<script setup lang="ts">
import { initialNavigation } from "~/composables/state-navigation.js";

definePageMeta({
    layout: false
})

// Blank bootstrap page: app.vue navigates away from it on startup. If
// anything ever lands back here (e.g. a history entry from before startup),
// send the user on to where they belong instead of a white screen.
const state = useAppState();
onMounted(() => {
  if (state.value) initialNavigation(state);
});
</script>
