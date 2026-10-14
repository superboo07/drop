<script setup lang="ts">
import { WrenchIcon } from "@heroicons/vue/20/solid";

const jobs = useWinetricksJobs();
const running = computed(() =>
  Object.values(jobs.value).filter((job) => job.state === "Running"),
);
const job = computed(() => running.value[0]);
const now = useNow();

// The name for the popover
const gameName = ref<string | undefined>();
watch(
  () => job.value?.gameId,
  async (gameId) => {
    gameName.value = undefined;
    if (!gameId) return;
    try {
      const { game } = await useGame(gameId);
      if (job.value?.gameId === gameId) gameName.value = game.mName;
    } catch {
      // Shown without a name
    }
  },
  { immediate: true },
);

const verb = computed(() =>
  job.value ? currentVerb(job.value) : undefined,
);
const label = computed(() => {
  if (!job.value) return "";
  const what = verb.value
    ? `installing ${verb.value.name}` +
      (job.value.verbs.length > 1 ? ` (${verbPosition(job.value)})` : "")
    : "starting up";
  return `Winetricks · ${what}`;
});
</script>

<template>
  <li v-if="job" class="relative group">
    <NuxtLink
      :to="{ path: `/library/${job.gameId}`, query: { options: 'Proton' } }"
      class="transition inline-flex items-center cursor-pointer rounded-sm px-4 py-1.5 bg-zinc-900 hover:bg-zinc-800 relative h-full overflow-hidden"
    >
      <WrenchIcon class="relative z-10 h-5 text-white" aria-hidden="true" />
      <span class="sr-only">{{ gameName ?? "A game" }}: {{ label }}</span>
      <div
        class="transition-all absolute left-0 top-0 bottom-0 bg-blue-600"
        :style="{ width: `${jobFraction(job) * 99 + 1}%` }"
      />
    </NuxtLink>
    <div
      role="tooltip"
      class="pointer-events-none invisible opacity-0 transition group-hover:visible group-hover:opacity-100 absolute right-0 top-full mt-2 z-[500] w-72 rounded-md bg-zinc-900 px-3.5 py-3 shadow-lg ring-1 ring-zinc-100/10 space-y-2"
    >
      <div class="flex justify-between gap-3 text-sm">
        <span class="font-semibold text-zinc-100 truncate">{{
          gameName ?? "Winetricks"
        }}</span>
        <span class="font-mono text-zinc-400 tabular-nums">{{
          formatDuration(now - job.startedAt)
        }}</span>
      </div>
      <p class="text-sm text-zinc-300">{{ label }}</p>
      <p v-if="verb?.activity" class="text-xs text-zinc-400 truncate">
        {{ verb.activity
        }}<template v-if="verb.percent !== null"> · {{ verb.percent }}%</template>
      </p>
      <div class="h-1 overflow-hidden rounded-full bg-zinc-800">
        <div
          class="h-full bg-blue-500 transition-all"
          :style="{ width: `${jobFraction(job) * 100}%` }"
        />
      </div>
      <p v-if="running.length > 1" class="text-xs text-zinc-400">
        And {{ running.length - 1 }} more game{{
          running.length > 2 ? "s" : ""
        }}
      </p>
      <p class="text-xs text-zinc-500">Click to open the game's Proton options</p>
    </div>
  </li>
</template>
