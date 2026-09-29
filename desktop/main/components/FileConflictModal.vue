<template>
  <ModalTemplate v-model="open" size-class="sm:max-w-2xl">
    <template #default>
      <div v-if="current" class="flex items-start gap-4">
        <span
          class="grid size-10 shrink-0 place-items-center rounded-full bg-orange-500/15 text-orange-400"
        >
          <ExclamationTriangleIcon class="size-6" />
        </span>
        <div class="min-w-0">
          <h3 class="text-lg font-semibold text-zinc-100">
            Update {{ current.gameName }}
          </h3>
          <p class="mt-1 text-sm text-zinc-400">
            You've changed {{ current.conflicts.length }}
            {{ current.conflicts.length === 1 ? "file" : "files" }} that this
            update also changes or removes. Choose what to do with each one.
          </p>
        </div>
      </div>

      <div
        v-if="current"
        class="max-h-80 overflow-y-auto rounded-lg bg-zinc-950 ring-1 ring-zinc-800"
      >
        <table class="w-full text-sm">
          <thead class="sticky top-0 bg-zinc-950">
            <tr class="text-xs uppercase tracking-wider text-zinc-500">
              <th class="px-3 py-2 text-center font-semibold">Overwrite</th>
              <th class="px-3 py-2 text-center font-semibold">Keep mine</th>
              <th class="px-3 py-2 text-left font-semibold">File</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-zinc-800">
            <tr v-for="conflict in current.conflicts" :key="conflict.path">
              <td class="px-3 py-2 text-center">
                <input
                  v-model="choices[conflict.path]"
                  type="radio"
                  value="Overwrite"
                  :name="conflict.path"
                  :aria-label="`${overwriteLabel(conflict)} ${conflict.path}`"
                  class="size-4 accent-blue-600"
                />
              </td>
              <td class="px-3 py-2 text-center">
                <input
                  v-model="choices[conflict.path]"
                  type="radio"
                  value="Keep"
                  :name="conflict.path"
                  :aria-label="`Keep my ${conflict.path}`"
                  class="size-4 accent-blue-600"
                />
              </td>
              <td class="px-3 py-2 min-w-0">
                <span class="block truncate font-mono text-zinc-200">{{
                  conflict.path
                }}</span>
                <span
                  v-if="conflict.kind === 'Removed'"
                  class="text-xs text-zinc-500"
                  >Removed in this version. Overwriting deletes it.</span
                >
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <div v-if="current" class="flex flex-wrap items-center gap-2">
        <button
          type="button"
          class="rounded-md bg-zinc-800 px-3 py-1.5 text-sm font-semibold text-zinc-100 hover:bg-zinc-700"
          @click="setAll('Overwrite')"
        >
          Overwrite all
        </button>
        <button
          type="button"
          class="rounded-md bg-zinc-800 px-3 py-1.5 text-sm font-semibold text-zinc-100 hover:bg-zinc-700"
          @click="setAll('Keep')"
        >
          Keep all
        </button>
        <p class="text-xs text-zinc-500">
          Files you keep may not work with the new version.
        </p>
      </div>

      <p v-if="error" class="text-sm text-red-400">{{ error }}</p>
    </template>
    <template #buttons>
      <LoadingButton
        :loading="submitting"
        :disabled="!allAnswered"
        class="w-full sm:w-auto"
        @click="submit"
      >
        Continue
      </LoadingButton>
      <button
        type="button"
        class="mt-3 inline-flex w-full justify-center rounded-md bg-zinc-900 px-3 py-2 text-sm font-semibold text-zinc-100 ring-1 ring-inset ring-zinc-700 hover:bg-zinc-950 sm:mt-0 sm:w-auto"
        @click="cancelDownload"
      >
        Cancel update
      </button>
    </template>
  </ModalTemplate>
</template>

<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ExclamationTriangleIcon } from "@heroicons/vue/24/outline";
import type { DownloadableMetadata } from "~/types";

type Choice = "Overwrite" | "Keep";
type FileConflict = { path: string; kind: "Changed" | "Removed" };
type PendingConflicts = {
  meta: DownloadableMetadata;
  gameName: string;
  conflicts: FileConflict[];
};

// Downloads waiting on the player, oldest first; one is shown at a time.
const pending = ref<PendingConflicts[]>([]);
const current = computed(() => pending.value[0]);
const choices = ref<Record<string, Choice>>({});
const submitting = ref(false);
const error = ref<string | undefined>();

// Only closed by answering or cancelling: dismissing would leave the
// download waiting with nothing on screen.
const open = computed({
  get: () => current.value !== undefined,
  set: () => {},
});

const allAnswered = computed(
  () =>
    current.value?.conflicts.every((c) => choices.value[c.path]) ?? false,
);

function overwriteLabel(conflict: FileConflict) {
  return conflict.kind === "Removed" ? "Delete" : "Overwrite";
}

function setAll(choice: Choice) {
  for (const conflict of current.value?.conflicts ?? [])
    choices.value[conflict.path] = choice;
}

function add(entry: PendingConflicts) {
  pending.value = [
    ...pending.value.filter((p) => p.meta.id !== entry.meta.id),
    entry,
  ];
}

function remove(gameId: string) {
  pending.value = pending.value.filter((p) => p.meta.id !== gameId);
}

watch(current, () => {
  choices.value = {};
  error.value = undefined;
});

async function submit() {
  if (!current.value) return;
  submitting.value = true;
  error.value = undefined;
  try {
    await invoke("resolve_file_conflicts", {
      gameId: current.value.meta.id,
      choices: choices.value,
    });
    remove(current.value.meta.id);
  } catch (e) {
    error.value = String(e);
  } finally {
    submitting.value = false;
  }
}

async function cancelDownload() {
  if (!current.value) return;
  const meta = current.value.meta;
  try {
    await invoke("cancel_game", { meta });
  } finally {
    remove(meta.id);
  }
}

listen("download_file_conflicts", (event) => {
  add(event.payload as PendingConflicts);
});
listen("download_file_conflicts_cleared", (event) => {
  remove(event.payload as string);
});

onMounted(async () => {
  try {
    const existing = await invoke<PendingConflicts[]>("fetch_file_conflicts");
    existing.forEach(add);
  } catch (e) {
    console.error("failed to fetch pending file conflicts", e);
  }
});
</script>
