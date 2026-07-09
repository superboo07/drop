<template>
  <div class="min-h-full w-full flex flex-col justify-center px-6 py-6">
    <div class="mb-4">
      <h1 class="text-lg font-semibold font-display text-zinc-100">
        Choose how to launch
      </h1>
      <p class="mt-1 text-sm text-zinc-400">
        This game has multiple launch options configured. Select one to
        start.
      </p>
    </div>

    <p v-if="error" class="text-sm text-red-400">
      {{ error }}
    </p>

    <ol v-else class="space-y-2">
      <li v-for="(option, index) in launchOptions" :key="index">
        <button
          type="button"
          class="transition w-full rounded-sm bg-zinc-800 inline-flex items-center text-sm py-2 px-3 gap-x-2 text-zinc-100 hover:text-zinc-300 hover:bg-zinc-700 disabled:opacity-50"
          :disabled="launching"
          @click="() => choose(index)"
        >
          <PlayIcon class="size-4" />
          <span>{{ option.name }}</span>
        </button>
      </li>
    </ol>

    <button
      type="button"
      class="mt-4 inline-flex w-full justify-center rounded-md bg-zinc-800 px-3 py-2 text-sm font-semibold text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-700 hover:bg-zinc-900"
      :disabled="launching"
      @click="() => getCurrentWindow().close()"
    >
      Cancel
    </button>
  </div>
</template>

<script setup lang="ts">
import { PlayIcon } from "@heroicons/vue/20/solid";
import { getCurrentWindow } from "@tauri-apps/api/window";

definePageMeta({
  layout: "mini",
});

const route = useRoute();
const gameId = String(route.query.id ?? "");

const launchOptions = ref<Array<{ name: string }>>([]);
const error = ref<string | undefined>();
const launching = ref(false);

async function loadOptions() {
  if (!gameId) {
    error.value = "No game specified.";
    return;
  }
  try {
    launchOptions.value = await invokeWithTimeout<Array<{ name: string }>>(
      "get_launch_options",
      { id: gameId },
    );
    if (launchOptions.value.length === 0) {
      error.value = "This game has no launch options configured.";
    }
  } catch (e) {
    error.value = `Couldn't load launch options: ${e}`;
  }
}

async function choose(index: number) {
  if (launching.value) return;
  launching.value = true;
  try {
    await invokeWithTimeout<LaunchResult>("launch_game", {
      id: gameId,
      index,
    });
  } catch (e) {
    error.value = `Drop failed to launch this game: ${e}`;
    launching.value = false;
    return;
  }
  await getCurrentWindow().close();
}

onMounted(loadOptions);
</script>
