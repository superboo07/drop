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

    <ol v-else class="space-y-3">
      <li v-for="(option, index) in launchOptions" :key="index">
        <button
          :ref="(el: Element | ComponentPublicInstance | null) => setButtonRef(el, index)"
          type="button"
          class="transition w-full rounded-md bg-zinc-800 inline-flex items-center text-base py-4 px-4 gap-x-3 text-zinc-100 hover:text-zinc-300 hover:bg-zinc-700 disabled:opacity-50 ring-1 ring-inset ring-zinc-700 focus:bg-zinc-700"
          :disabled="launching"
          @click="() => choose(index)"
        >
          <PlayIcon class="size-6 shrink-0" />
          <span>{{ option.name }}</span>
        </button>
      </li>
    </ol>

    <button
      type="button"
      class="mt-4 inline-flex w-full justify-center rounded-md bg-zinc-800 px-4 py-3 text-base font-semibold text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-700 hover:bg-zinc-900"
      :disabled="launching"
      @click="() => cancel()"
    >
      Cancel
    </button>
  </div>
</template>

<script setup lang="ts">
import { PlayIcon } from "@heroicons/vue/20/solid";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ComponentPublicInstance } from "vue";

definePageMeta({
  layout: "mini",
});

const route = useRoute();
const gameId = String(route.query.id ?? "");

const launchOptions = ref<Array<{ name: string }>>([]);
const error = ref<string | undefined>();
const launching = ref(false);
const buttonRefs = ref<HTMLButtonElement[]>([]);

function setButtonRef(el: Element | ComponentPublicInstance | null, index: number) {
  if (el instanceof HTMLButtonElement) buttonRefs.value[index] = el;
}

// Gamepad input is handled app-wide by useSpatialGamepadNavigation (mounted
// in app.vue, which this window also loads) -- it moves real DOM focus, so
// this just needs to move focus the same way for the keyboard.
function moveSelection(delta: -1 | 1) {
  const buttons = buttonRefs.value;
  const count = buttons.length;
  if (count === 0) return;
  const currentIndex = buttons.indexOf(document.activeElement as HTMLButtonElement);
  const nextIndex = currentIndex === -1 ? 0 : (currentIndex + delta + count) % count;
  buttons[nextIndex]?.focus();
}

function cancel() {
  getCurrentWindow().close();
}

function onKeydown(e: KeyboardEvent) {
  switch (e.key) {
    case "ArrowUp":
      moveSelection(-1);
      break;
    case "ArrowDown":
      moveSelection(1);
      break;
    case "Enter":
      (document.activeElement as HTMLElement | null)?.click();
      break;
    case "Escape":
      cancel();
      break;
    default:
      return;
  }
  e.preventDefault();
}

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
    } else {
      await nextTick();
      buttonRefs.value[0]?.focus();
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

onMounted(() => {
  loadOptions();
  window.addEventListener("keydown", onKeydown);
});
onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
});
</script>
