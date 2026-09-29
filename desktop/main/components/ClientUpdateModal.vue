<template>
  <ModalTemplate v-model="open" size-class="sm:max-w-xl">
    <template #default>
      <!-- installed -->
      <div v-if="phase === 'done'" class="flex items-start gap-4">
        <span
          class="grid size-10 shrink-0 place-items-center rounded-full bg-green-500/15 text-green-400"
        >
          <CheckIcon class="size-6" />
        </span>
        <div>
          <h3 class="text-lg font-semibold text-zinc-100">
            Drop {{ status?.installed }} is installed
          </h3>
          <p class="mt-1 text-sm text-zinc-400">
            Restart Drop to start using it. If a game is running, restart after
            you close it.
          </p>
        </div>
      </div>

      <template v-else-if="update">
        <div class="flex items-start gap-4">
          <span
            class="grid size-10 shrink-0 place-items-center rounded-full bg-blue-500/15 text-blue-400"
          >
            <ArrowUpCircleIcon class="size-6" />
          </span>
          <div class="min-w-0">
            <h3 class="text-lg font-semibold text-zinc-100">{{ title }}</h3>
            <p class="mt-1 text-sm text-zinc-400">
              {{ subtitle }}
            </p>
          </div>
        </div>

        <div
          v-if="update.required && phase === 'idle'"
          class="rounded-md bg-orange-500/10 px-4 py-3 text-sm text-orange-200 ring-1 ring-orange-500/20"
        >
          Your server requires this update. Drop may not work with it until
          you update.
        </div>

        <!-- notes -->
        <div
          v-if="phase === 'idle' || phase === 'failed'"
          class="max-h-72 overflow-y-auto rounded-lg bg-zinc-950 p-4 ring-1 ring-zinc-800"
        >
          <div
            v-for="entry in update.notes"
            :key="entry.id"
            class="[&+&]:mt-5"
          >
            <div class="text-xs font-semibold text-zinc-500">
              <!-- the tag keeps its exact case: it's a version string -->
              <span class="font-mono text-zinc-400">{{ entry.tag }}</span>
              ·
              <span class="uppercase tracking-wider">{{
                formatDate(entry.publishedAt)
              }}</span>
            </div>
            <div
              v-if="entry.notes"
              class="prose prose-sm prose-invert prose-blue mt-1 max-w-none"
              v-html="render(entry.notes)"
            />
            <p v-else class="mt-1 text-sm text-zinc-500">No patch notes.</p>
          </div>
        </div>

        <!-- progress -->
        <div v-if="phase === 'working'" class="flex flex-col gap-3">
          <div class="flex justify-between text-sm text-zinc-300 tabular-nums">
            <span>{{ progressLabel }}</span>
            <span v-if="progress?.phase === 'Downloading'">
              {{ formatMB(progress.downloaded) }} of
              {{ formatMB(progress.total) }}
            </span>
          </div>
          <div class="h-1.5 overflow-hidden rounded-full bg-zinc-800">
            <div
              class="h-full rounded-full bg-blue-500 transition-[width]"
              :style="{ width: `${Math.round(fraction * 100)}%` }"
            />
          </div>
        </div>

        <div
          v-if="phase === 'failed'"
          class="flex gap-3 rounded-md bg-red-600/10 p-4 text-sm text-red-300"
        >
          <ExclamationTriangleIcon class="size-5 shrink-0 text-red-400" />
          <span>{{ failure }} Your current version still works.</span>
        </div>

        <p
          v-if="status?.appimagePath && phase !== 'working'"
          class="text-xs text-zinc-500"
        >
          Replaces
          <span class="break-all font-mono text-zinc-400">{{
            status.appimagePath
          }}</span
          >. Running games aren't affected until Drop restarts.
        </p>
      </template>
    </template>

    <template #buttons>
      <template v-if="phase === 'done'">
        <LoadingButton
          :loading="restarting"
          class="w-full sm:w-fit"
          @click="restart"
        >
          Restart now
        </LoadingButton>
        <button type="button" :class="secondary" @click="open = false">
          Later
        </button>
      </template>
      <template v-else-if="phase === 'working'">
        <button
          v-if="progress?.phase === 'Downloading' || !progress"
          type="button"
          :class="secondary"
          @click="cancel"
        >
          Cancel
        </button>
      </template>
      <template v-else>
        <LoadingButton
          :loading="false"
          class="w-full sm:w-fit"
          @click="install"
        >
          {{ phase === "failed" ? "Try again" : actionLabel }}
        </LoadingButton>
        <button
          v-if="!update?.required"
          type="button"
          :class="secondary"
          @click="open = false"
        >
          Later
        </button>
        <button v-else type="button" :class="secondary" @click="quit">
          Quit Drop
        </button>
      </template>
    </template>
  </ModalTemplate>
</template>

<script setup lang="ts">
/* eslint-disable vue/no-v-html */
import { micromark } from "micromark";
import {
  ArrowUpCircleIcon,
  CheckIcon,
  ExclamationTriangleIcon,
} from "@heroicons/vue/24/outline";

const dialog = useClientUpdateDialog();
const status = useClientUpdateStatus();
const progress = useClientUpdateProgress();

const update = computed(() => status.value?.update);
const working = ref(false);
const failure = ref<string | undefined>();
const restarting = ref(false);

const phase = computed(() => {
  if (status.value?.installed) return "done";
  if (working.value) return "working";
  if (failure.value) return "failed";
  return "idle";
});

// A required update can't be dismissed, and nothing closes mid-install
const open = computed({
  get: () => dialog.value,
  set: (value) => {
    if (!value && phase.value === "working") return;
    if (!value && update.value?.required && phase.value !== "done") return;
    dialog.value = value;
  },
});

const branchLabel = (branch?: string) =>
  branch === "test" ? "test branch" : "release branch";

const title = computed(() => {
  const u = update.value!;
  if (u.rollback) return `Switch to Drop ${u.tag}`;
  if (status.value?.current?.tag === u.tag)
    return `An updated build of Drop ${u.tag} is available`;
  return `Drop ${u.tag} is available`;
});

const subtitle = computed(() => {
  const u = update.value!;
  const have = status.value?.current?.tag ?? status.value?.appVersion;
  const size = `Download size ${formatMB(u.size)}.`;
  if (u.rollback)
    return `You have ${have}. ${u.tag} is the newest build on the ${branchLabel(status.value?.branch)}, and it's older than what you have. ${size}`;
  return `You have ${have}. ${size}`;
});

const actionLabel = computed(() =>
  update.value?.rollback ? "Switch and restart" : "Update and restart",
);

const progressLabel = computed(() => {
  switch (progress.value?.phase) {
    case "Verifying":
      return "Checking the download…";
    case "Replacing":
      return "Installing…";
    default:
      return "Downloading…";
  }
});

const fraction = computed(() => {
  const p = progress.value;
  if (p?.phase === "Downloading") return p.total ? p.downloaded / p.total : 0;
  if (p?.phase === "Verifying" || p?.phase === "Replacing") return 1;
  return 0;
});

async function install() {
  failure.value = undefined;
  working.value = true;
  try {
    await installClientUpdate();
    // "Done" arrives as a status change (installed); "Cancelled" just ends
  } catch (e) {
    failure.value = typeof e === "string" ? e : String(e);
  } finally {
    working.value = false;
  }
}

async function cancel() {
  await cancelClientUpdate();
}

async function restart() {
  restarting.value = true;
  try {
    await restartAfterClientUpdate();
  } catch (e) {
    restarting.value = false;
    failure.value = typeof e === "string" ? e : String(e);
  }
}

async function quit() {
  await invokeWithTimeout("quit");
}

function render(markdown: string) {
  return micromark(markdown);
}

function formatMB(bytes: number) {
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function formatDate(date: string) {
  return new Date(date).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

const secondary =
  "mt-3 inline-flex w-full justify-center rounded-md bg-zinc-800 px-3 py-2 text-sm font-semibold text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-700 hover:bg-zinc-900 sm:mt-0 sm:w-auto";
</script>
