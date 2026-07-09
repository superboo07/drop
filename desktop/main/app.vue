<template>
  <div
    v-if="startupError"
    class="w-screen h-screen flex items-center justify-center bg-zinc-950 text-zinc-100 p-8"
  >
    <div class="max-w-2xl">
      <h1 class="text-xl font-semibold mb-2">Drop failed to start</h1>
      <pre class="whitespace-pre-wrap text-sm text-red-400">{{ startupError }}</pre>
    </div>
  </div>
  <template v-else>
    <NuxtLoadingIndicator color="#2563eb" />
    <NuxtLayout class="select-none w-screen h-screen">
      <NuxtPage />
      <ModalStack />
    </NuxtLayout>
  </template>
</template>

<script setup lang="ts">
import "~/composables/downloads.js";

import { useAppState } from "./composables/app-state.js";
import {
  initialNavigation,
  setupHooks,
} from "./composables/state-navigation.js";
import { listen } from "@tauri-apps/api/event";
import type { AppState } from "./types.js";

const router = useRouter();

useSpatialGamepadNavigation();

const state = useAppState();
const startupError = ref<string | undefined>();

async function fetchState() {
  state.value = JSON.parse(await invokeWithTimeout("fetch_state"));
  if (!state.value) throw new Error(`App state is: ${state.value}`);
}

try {
  await fetchState();

  listen("update_state", (event) => {
    state.value = event.payload as AppState;
  });

  listen("window_visibility_change", () => {
    document.dispatchEvent(new Event("visibilitychange"));
  });

  setupHooks();
  await initialNavigation(state);
} catch (e) {
  console.error("failed to start up", e);
  startupError.value = e instanceof Error ? `${e.message}\n${e.stack}` : String(e);
}

useHead({
  title: "Drop",
});
</script>
