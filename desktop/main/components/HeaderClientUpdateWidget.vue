<script setup lang="ts">
import { ArrowUpCircleIcon } from "@heroicons/vue/20/solid";

const show = useShowClientUpdateWidget();
const status = useClientUpdateStatus();
const dialog = useClientUpdateDialog();

const label = computed(() =>
  status.value?.installed
    ? `Drop ${status.value.installed} is installed. Restart to use it.`
    : `Drop ${status.value?.update?.tag} is available`,
);
</script>

<template>
  <li v-if="show">
    <button
      type="button"
      :title="label"
      class="transition inline-flex items-center cursor-pointer rounded-sm px-4 py-1.5 bg-blue-500/15 ring-1 ring-blue-400/40 text-blue-400 hover:bg-blue-500/25 relative h-full"
      @click="dialog = true"
    >
      <ArrowUpCircleIcon class="h-5" />
      <span class="sr-only">{{ label }}</span>
      <span
        class="absolute top-0 right-0 translate-x-[30%] translate-y-[-30%] size-2.5 rounded-full bg-blue-400"
      />
    </button>
  </li>
</template>
