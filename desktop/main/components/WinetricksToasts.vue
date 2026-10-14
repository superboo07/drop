<script setup lang="ts">
import {
  CheckCircleIcon,
  ExclamationCircleIcon,
  XMarkIcon,
} from "@heroicons/vue/20/solid";
import type { WinetricksJob } from "~/types";

const finished = useWinetricksFinished();
const names = ref<Record<string, string>>({});
const TOAST_MS = 10_000;
const timers = new Map<string, ReturnType<typeof setTimeout>>();

watch(
  finished,
  (jobs) => {
    for (const job of jobs) {
      if (!names.value[job.gameId])
        useGame(job.gameId)
          .then(({ game }) => (names.value[job.gameId] = game.mName))
          .catch(() => {});
      // Failures stay up until they're dealt with
      if (job.state === "Succeeded" && !timers.has(job.gameId))
        timers.set(
          job.gameId,
          setTimeout(() => hide(job.gameId), TOAST_MS),
        );
    }
  },
  { immediate: true, deep: true },
);

// Hides the toast; the result stays on the game's Proton tab
function hide(gameId: string) {
  clearTimeout(timers.get(gameId));
  timers.delete(gameId);
  finished.value = finished.value.filter((job) => job.gameId !== gameId);
}

function summary(job: WinetricksJob) {
  if (job.state === "Succeeded")
    return `Winetricks installed ${job.verbs.map((v) => v.name).join(", ")}`;
  return job.error ?? "Winetricks failed";
}
</script>

<template>
  <div
    class="pointer-events-none fixed bottom-4 right-4 z-[600] flex w-80 flex-col gap-2"
    aria-live="polite"
  >
    <TransitionGroup
      enter-active-class="transition ease-out duration-200"
      enter-from-class="opacity-0 translate-y-2"
      leave-active-class="transition ease-in duration-150"
      leave-to-class="opacity-0"
    >
      <div
        v-for="job in finished"
        :key="job.gameId"
        role="status"
        :class="[
          'pointer-events-auto flex gap-3 rounded-lg bg-zinc-800 px-3.5 py-3 shadow-xl ring-1',
          job.state === 'Succeeded' ? 'ring-white/10' : 'ring-red-500/40',
        ]"
      >
        <CheckCircleIcon
          v-if="job.state === 'Succeeded'"
          class="mt-0.5 size-5 shrink-0 text-green-500"
        />
        <ExclamationCircleIcon
          v-else
          class="mt-0.5 size-5 shrink-0 text-red-400"
        />
        <div class="min-w-0 flex-1 space-y-1">
          <p class="text-sm font-semibold text-zinc-100">
            {{
              job.state === "Succeeded"
                ? `${names[job.gameId] ?? "Your game"} is ready to play`
                : `Winetricks failed for ${names[job.gameId] ?? "a game"}`
            }}
          </p>
          <p class="text-xs text-zinc-400 break-words">{{ summary(job) }}</p>
          <div class="flex gap-4 pt-1 text-xs font-semibold">
            <NuxtLink
              :to="
                job.state === 'Succeeded'
                  ? `/library/${job.gameId}`
                  : {
                      path: `/library/${job.gameId}`,
                      query: { options: 'Proton' },
                    }
              "
              class="text-blue-400 hover:text-blue-300"
              @click="() => hide(job.gameId)"
            >
              {{ job.state === "Succeeded" ? "Go to game" : "See what happened" }}
            </NuxtLink>
          </div>
        </div>
        <button
          type="button"
          class="self-start text-zinc-500 hover:text-zinc-300"
          aria-label="Close"
          @click="() => hide(job.gameId)"
        >
          <XMarkIcon class="size-4" />
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>
