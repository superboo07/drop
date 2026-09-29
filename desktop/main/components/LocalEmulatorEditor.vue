<template>
  <div>
    <div class="flex flex-row flex-wrap items-center gap-3">
      <span
        v-if="model?.kind === 'flatpak'"
        class="grow break-all rounded-md bg-white/5 p-2 text-sm text-zinc-100"
      >
        <span
          class="mr-2 rounded bg-blue-500/15 px-1.5 py-0.5 text-xs font-semibold text-blue-300"
          >Flatpak</span
        >
        {{ nameOf(model.path) }}
        <span class="font-mono text-zinc-400">{{ model.path }}</span>
      </span>
      <span
        v-else-if="model"
        class="grow break-all rounded-md bg-white/5 p-2 font-mono text-sm text-zinc-100"
        >{{ model.path }}</span
      >
      <span v-else class="grow text-sm italic text-zinc-400">{{
        emptyText
      }}</span>
      <div class="flex shrink-0 flex-row items-center gap-x-2">
        <button
          @click="pickExecutable"
          type="button"
          class="inline-flex items-center gap-x-2 rounded-md bg-zinc-800 px-3 py-2 text-sm font-semibold text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-700 hover:bg-zinc-900"
        >
          Select executable
        </button>
        <button
          v-if="flatpakSupported"
          @click="toggleFlatpakPicker"
          type="button"
          class="inline-flex items-center gap-x-2 rounded-md bg-zinc-800 px-3 py-2 text-sm font-semibold text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-700 hover:bg-zinc-900"
        >
          Choose Flatpak
        </button>
        <button
          v-if="model"
          @click="() => (model = undefined)"
          type="button"
          class="ml-1 text-sm font-semibold text-red-400 hover:text-red-300"
        >
          Remove
        </button>
      </div>
    </div>

    <div v-if="pickerOpen" class="mt-3">
      <p v-if="apps === undefined" class="text-sm text-zinc-400">
        Looking for installed Flatpaks…
      </p>
      <p v-else-if="apps.length === 0" class="text-sm italic text-zinc-400">
        No Flatpak apps are installed (or Flatpak isn't available).
      </p>
      <template v-else>
        <input
          v-model="filter"
          type="text"
          placeholder="Search Flatpaks"
          class="block w-full rounded-md bg-white/5 p-2 text-sm text-white outline-1 -outline-offset-1 outline-white/10 focus:outline-2 focus:-outline-offset-2 focus:outline-blue-500"
        />
        <ul
          role="list"
          class="mt-2 max-h-64 overflow-y-auto rounded-md bg-zinc-900 ring-1 ring-zinc-800"
        >
          <li v-for="app in filteredApps" :key="app.id">
            <button
              type="button"
              @click="() => pickFlatpak(app.id)"
              class="flex w-full flex-row items-baseline gap-x-2 px-3 py-2 text-left text-sm hover:bg-zinc-800"
            >
              <span class="font-semibold text-zinc-100">{{ app.name }}</span>
              <span class="truncate font-mono text-xs text-zinc-400">{{
                app.id
              }}</span>
            </button>
          </li>
          <li
            v-if="filteredApps.length === 0"
            class="px-3 py-2 text-sm italic text-zinc-400"
          >
            No matches.
          </li>
        </ul>
      </template>
    </div>

    <div v-if="model" class="mt-3">
      <label :for="argsId" class="block text-xs font-medium text-zinc-400"
        >Arguments</label
      >
      <input
        :id="argsId"
        :value="model.args"
        @change="setArgs"
        type="text"
        placeholder="{rom}"
        class="mt-1 block w-full rounded-md bg-white/5 p-2 font-mono text-sm text-white outline-1 -outline-offset-1 outline-white/10 focus:outline-2 focus:-outline-offset-2 focus:outline-blue-500"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { open } from "@tauri-apps/plugin-dialog";
import { platform } from "@tauri-apps/plugin-os";
import type { LocalEmulator } from "~/types";

defineProps<{ emptyText: string }>();
const model = defineModel<LocalEmulator | undefined>();

const argsId = useId();

// Flatpak is a Linux thing.
const flatpakSupported = platform() === "linux";
const { apps, load, nameOf } = useFlatpakApps();
if (model.value?.kind === "flatpak") load();

const pickerOpen = ref(false);
const filter = ref("");
const filteredApps = computed(() => {
  const needle = filter.value.trim().toLowerCase();
  return (apps.value ?? []).filter(
    (app) =>
      !needle ||
      app.name.toLowerCase().includes(needle) ||
      app.id.toLowerCase().includes(needle),
  );
});

function set(kind: LocalEmulator["kind"], path: string) {
  model.value = { kind, path, args: model.value?.args ?? "{rom}" };
}

async function pickExecutable() {
  pickerOpen.value = false;
  const file = await open({ multiple: false, directory: false });
  if (file) set("executable", file);
}

function toggleFlatpakPicker() {
  pickerOpen.value = !pickerOpen.value;
  filter.value = "";
  if (pickerOpen.value) load();
}

function pickFlatpak(appId: string) {
  pickerOpen.value = false;
  set("flatpak", appId);
}

function setArgs(event: Event) {
  if (!model.value) return;
  model.value = {
    ...model.value,
    args: (event.target as HTMLInputElement).value,
  };
}
</script>
