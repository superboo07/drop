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
import type { AppState, Settings } from "./types.js";

const router = useRouter();
const route = useRoute();

useSpatialGamepadNavigation();

const state = useAppState();
const startupError = ref<string | undefined>();

async function fetchState() {
  state.value = JSON.parse(await invokeWithTimeout("fetch_state"));
  if (!state.value) throw new Error(`App state is: ${state.value}`);
}

// User-controlled scale (Settings -> Interface). uiScaleRef feeds
// useLiveUiScale below, which keeps main.scss's root font-size correct as
// the viewport changes (gamescope resolution changes, window resizes, ...),
// not just once at startup.
const uiScaleRef = ref(1);

async function loadUiScale() {
  const settings = await invokeWithTimeout<Settings>("fetch_settings");
  // The launch picker is a small, sparse dialog (a handful of large,
  // isolated buttons on an otherwise empty screen) - it doesn't read the
  // same way the main window's dense UI does at the same scale, so a value
  // tuned for e.g. a TV makes it feel disproportionately huge. Only half of
  // any increase above 100% carries over to that window.
  const isLaunchPicker = route.path === "/launch-picker";
  uiScaleRef.value = isLaunchPicker
    ? 1 + (settings.uiScale - 1) * 0.5
    : settings.uiScale;
  // Applied immediately as a flat value (not yet knowing whether this
  // window is fullscreen-under-gamescope, which useLiveUiScale's onMounted
  // resolves and corrects for a moment later) so there's no flash of
  // completely unscaled content before the app renders below.
  document.documentElement.style.fontSize = `${16 * uiScaleRef.value}px`;
}

useLiveUiScale(uiScaleRef);

try {
  await Promise.all([fetchState(), loadUiScale()]);

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
