<template>
  <div class="border-b border-zinc-700 py-5">
    <h3 class="text-base font-semibold font-display leading-6 text-zinc-100">
      General
    </h3>
  </div>

  <div class="mt-5 space-y-8">
    <div class="flex flex-row items-center justify-between">
      <div>
        <h3 class="text-sm font-medium leading-6 text-zinc-100">
          Start with system
        </h3>
        <p class="mt-1 text-sm leading-6 text-zinc-400">
          Drop will automatically start when you log into your computer
        </p>
      </div>
      <Switch
        v-model="autostartEnabled"
        :class="[
          autostartEnabled ? 'bg-blue-600' : 'bg-zinc-700',
          'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out',
        ]"
      >
        <span
          :class="[
            autostartEnabled ? 'translate-x-5' : 'translate-x-0',
            'pointer-events-none relative inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out',
          ]"
        />
      </Switch>
    </div>

    <div class="flex flex-row items-center justify-between">
      <div>
        <h3 class="text-sm font-medium leading-6 text-zinc-100">
          Quit when closing window
        </h3>
        <p class="mt-1 text-sm leading-6 text-zinc-400">
          By default, Drop keeps running in the tray when you close the
          window. Enable this to fully quit instead.
        </p>
      </div>
      <Switch
        v-model="quitOnClose"
        :class="[
          quitOnClose ? 'bg-blue-600' : 'bg-zinc-700',
          'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out',
        ]"
      >
        <span
          :class="[
            quitOnClose ? 'translate-x-5' : 'translate-x-0',
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

defineProps<{}>();

const autostartEnabled = ref<boolean>(false);
const quitOnClose = ref<boolean>(false);
const windowedLaunchPicker = ref<boolean>(false);

// Load initial state
invokeWithTimeout("get_autostart_enabled").then((enabled) => {
  autostartEnabled.value = enabled as boolean;
});

invokeWithTimeout<Settings>("fetch_settings").then((settings) => {
  quitOnClose.value = settings.quitOnClose;
  windowedLaunchPicker.value = settings.windowedLaunchPicker;
});

// Watch for changes and update autostart
watch(autostartEnabled, async (newValue: boolean) => {
  try {
    await invokeWithTimeout("toggle_autostart", { enabled: newValue });
  } catch (error) {
    console.error("Failed to toggle autostart:", error);
    // Revert the toggle if it failed
    autostartEnabled.value = !newValue;
  }
});

watch(quitOnClose, async (newValue: boolean) => {
  try {
    await invokeWithTimeout("update_settings", {
      newSettings: { quitOnClose: newValue },
    });
  } catch (error) {
    console.error("Failed to update quit-on-close setting:", error);
    quitOnClose.value = !newValue;
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
