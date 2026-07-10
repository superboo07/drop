<template>
  <div class="border-b border-zinc-700 py-5">
    <h3 class="text-base font-semibold font-display leading-6 text-zinc-100">
      Interface
    </h3>
  </div>

  <div class="mt-5 space-y-8">
    <div class="flex flex-col gap-y-2">
      <div class="flex flex-row items-center justify-between">
        <h3 class="text-sm font-medium leading-6 text-zinc-100">UI Scale</h3>
        <span class="text-sm text-zinc-400">{{ Math.round(uiScale * 100) }}%</span>
      </div>
      <p class="text-sm leading-6 text-zinc-400">
        Scales the size of Drop's interface. If things look too small (e.g.
        on a 4K display without OS-level display scaling configured) or too
        large, adjust this instead.
      </p>
      <input
        type="range"
        min="0.75"
        max="3"
        step="0.05"
        :value="uiScale"
        @input="onScaleInput"
        @change="onScaleChange"
        class="w-full accent-blue-600"
      />
    </div>

    <div class="flex flex-row items-center justify-between">
      <div>
        <h3 class="text-sm font-medium leading-6 text-zinc-100">
          Start in fullscreen
        </h3>
        <p class="mt-1 text-sm leading-6 text-zinc-400">
          Drop's main window opens at a fixed size by default. Enable this
          to have it fill the entire screen on startup instead.
        </p>
      </div>
      <Switch
        v-model="startFullscreen"
        :class="[
          startFullscreen ? 'bg-blue-600' : 'bg-zinc-700',
          'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out',
        ]"
      >
        <span
          :class="[
            startFullscreen ? 'translate-x-5' : 'translate-x-0',
            'pointer-events-none relative inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out',
          ]"
        />
      </Switch>
    </div>

    <div class="flex flex-row items-center justify-between">
      <div>
        <h3 class="text-sm font-medium leading-6 text-zinc-100">
          Windowed launch picker
        </h3>
        <p class="mt-1 text-sm leading-6 text-zinc-400">
          The window for choosing a launch option (shown for games with
          multiple ways to start, e.g. from a Steam shortcut) opens
          fullscreen by default. Enable this to open it as a small window
          instead.
        </p>
      </div>
      <Switch
        v-model="windowedLaunchPicker"
        :class="[
          windowedLaunchPicker ? 'bg-blue-600' : 'bg-zinc-700',
          'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out',
        ]"
      >
        <span
          :class="[
            windowedLaunchPicker ? 'translate-x-5' : 'translate-x-0',
            'pointer-events-none relative inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out',
          ]"
        />
      </Switch>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Switch } from "@headlessui/vue";
import type { Settings } from "~/types";

const startFullscreen = ref<boolean>(false);
const windowedLaunchPicker = ref<boolean>(false);
const uiScale = ref<number>(1);

invokeWithTimeout<Settings>("fetch_settings").then((settings) => {
  startFullscreen.value = settings.startFullscreen;
  windowedLaunchPicker.value = settings.windowedLaunchPicker;
  uiScale.value = settings.uiScale;
});

// Reapplies main.scss's root font-size whenever uiScale changes (live
// preview while dragging the slider below) or the viewport changes.
useLiveUiScale(uiScale);

function onScaleInput(event: Event) {
  uiScale.value = Number((event.target as HTMLInputElement).value);
}

async function onScaleChange() {
  try {
    await invokeWithTimeout("update_settings", {
      newSettings: { uiScale: uiScale.value },
    });
  } catch (error) {
    console.error("Failed to update UI scale setting:", error);
  }
}

watch(startFullscreen, async (newValue: boolean) => {
  try {
    await invokeWithTimeout("update_settings", {
      newSettings: { startFullscreen: newValue },
    });
  } catch (error) {
    console.error("Failed to update start-fullscreen setting:", error);
    startFullscreen.value = !newValue;
  }
});

watch(windowedLaunchPicker, async (newValue: boolean) => {
  try {
    await invokeWithTimeout("update_settings", {
      newSettings: { windowedLaunchPicker: newValue },
    });
  } catch (error) {
    console.error("Failed to update windowed-launch-picker setting:", error);
    windowedLaunchPicker.value = !newValue;
  }
});
</script>
