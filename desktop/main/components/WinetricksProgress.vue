<template>
  <section
    :aria-label="title"
    :class="[
      'rounded-md px-4 py-3 outline outline-1 space-y-3',
      {
        Running: 'bg-blue-900/20 outline-blue-500/30',
        Succeeded: 'bg-green-900/20 outline-green-500/30',
        Failed: 'bg-red-900/20 outline-red-500/40',
        Cancelled: 'bg-zinc-800/50 outline-white/10',
      }[job.state],
    ]"
  >
    <div class="flex items-center justify-between gap-3">
      <div class="flex items-center gap-2.5 min-w-0">
        <Spinner v-if="job.state === 'Running'" class="size-4 shrink-0" />
        <CheckIcon
          v-else-if="job.state === 'Succeeded'"
          class="size-5 shrink-0 text-green-500"
        />
        <ExclamationCircleIcon
          v-else-if="job.state === 'Failed'"
          class="size-5 shrink-0 text-red-400"
        />
        <XMarkIcon v-else class="size-5 shrink-0 text-zinc-400" />
        <h3 class="text-sm font-semibold text-zinc-100 truncate">
          {{ title }}
          <span
            v-if="job.state === 'Running' && job.verbs.length > 1"
            class="font-medium text-zinc-400"
            >· {{ verbPosition(job) }}</span
          >
        </h3>
      </div>
      <span class="font-mono text-xs text-zinc-400 tabular-nums">{{
        formatDuration((job.finishedAt ?? now) - job.startedAt)
      }}</span>
    </div>

    <div
      v-if="job.state === 'Running'"
      class="grid gap-1"
      :style="{
        gridTemplateColumns: `repeat(${job.verbs.length}, minmax(0, 1fr))`,
      }"
      aria-hidden="true"
    >
      <div
        v-for="verb in job.verbs"
        :key="verb.name"
        :class="[
          'relative h-1.5 overflow-hidden rounded-full',
          verbIsDone(verb) ? 'bg-blue-500' : 'bg-zinc-700/70',
        ]"
      >
        <div
          v-if="verb.state === 'Running' && verb.percent !== null"
          class="absolute inset-y-0 left-0 rounded-full bg-blue-500 transition-[width]"
          :style="{ width: `${verb.percent}%` }"
        />
        <div
          v-else-if="verb.state === 'Running'"
          class="winetricks-indeterminate absolute inset-y-0 w-2/5 rounded-full bg-blue-500"
        />
      </div>
    </div>

    <ol class="space-y-1.5 text-sm">
      <li
        v-for="verb in job.verbs"
        :key="verb.name"
        class="flex items-center gap-2.5"
      >
        <CheckIcon
          v-if="verbIsDone(verb)"
          class="size-4 shrink-0 text-green-500"
        />
        <Spinner v-else-if="verb.state === 'Running'" class="size-4 shrink-0" />
        <XMarkIcon
          v-else-if="verb.state === 'Failed'"
          class="size-4 shrink-0 text-red-400"
        />
        <span
          v-else
          class="size-4 shrink-0 flex items-center justify-center"
          aria-hidden="true"
        >
          <span class="size-3 rounded-full border-2 border-zinc-600" />
        </span>
        <span
          :class="[
            'w-28 shrink-0 truncate font-mono text-xs',
            verb.state === 'Waiting' || verb.state === 'NotRun'
              ? 'text-zinc-400'
              : 'text-zinc-100',
          ]"
          >{{ verb.name }}</span
        >
        <span
          :class="[
            'flex-1 truncate',
            verb.state === 'Running'
              ? 'text-zinc-300'
              : verb.state === 'Failed'
                ? 'text-red-300'
                : 'text-zinc-400',
          ]"
          >{{ verbLabel(verb) }}</span
        >
        <span class="font-mono text-xs text-zinc-500 tabular-nums">{{
          verbTime(verb)
        }}</span>
      </li>
    </ol>

    <p v-if="job.error" class="text-sm text-red-300">{{ job.error }}</p>

    <pre
      v-if="showOutput && job.output.length > 0"
      aria-label="Latest winetricks output"
      class="max-h-36 overflow-y-auto whitespace-pre-wrap break-all rounded bg-zinc-950 px-3 py-2 font-mono text-[11px] leading-relaxed text-zinc-400 select-text"
      >{{ job.output.join("\n") }}</pre
    >

    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="flex gap-4 text-sm">
        <button
          v-if="job.output.length > 0"
          type="button"
          class="font-medium text-blue-400 hover:text-blue-300"
          @click="showOutput = !showOutput"
        >
          {{ showOutput ? "Hide output" : "Show output" }}
        </button>
        <button
          type="button"
          class="font-medium text-blue-400 hover:text-blue-300"
          @click="() => openLog()"
        >
          Open full log
        </button>
      </div>
      <div class="flex items-center gap-2">
        <button
          v-if="job.state === 'Failed' && retryVerbs.length > 0"
          type="button"
          class="rounded-md bg-blue-600 px-2.5 py-1 text-xs font-semibold text-white hover:bg-blue-500"
          @click="() => retry()"
        >
          Retry {{ retryVerbs.length === 1 ? retryVerbs[0] : "failed" }}
        </button>
        <button
          v-if="job.state === 'Running'"
          type="button"
          :disabled="cancelling"
          class="rounded-md bg-zinc-800 px-2.5 py-1 text-xs font-semibold text-zinc-100 ring-1 ring-inset ring-zinc-700 hover:bg-zinc-700 disabled:opacity-60"
          @click="() => cancel()"
        >
          {{ cancelling ? "Cancelling…" : "Cancel" }}
        </button>
        <button
          v-else
          type="button"
          class="rounded-md px-2.5 py-1 text-xs font-semibold text-zinc-400 hover:text-zinc-200"
          @click="() => dismissWinetricks(job.gameId)"
        >
          Dismiss
        </button>
      </div>
    </div>

    <p v-if="actionError" class="text-sm text-red-400">{{ actionError }}</p>
    <p v-if="job.state === 'Running'" class="text-xs text-zinc-400">
      You can close this window; it keeps running and the game page shows its
      progress.
    </p>
  </section>
</template>

<script setup lang="ts">
import {
  CheckIcon,
  ExclamationCircleIcon,
  XMarkIcon,
} from "@heroicons/vue/20/solid";
import type { WinetricksJob, WinetricksVerbProgress } from "~/types";
import Spinner from "./WinetricksSpinner.vue";

const props = defineProps<{ job: WinetricksJob }>();

const now = useNow();
const showOutput = ref(props.job.state === "Failed");
const cancelling = ref(false);
const actionError = ref<string | undefined>();

watch(
  () => props.job.state,
  (state) => {
    cancelling.value = false;
    if (state === "Failed") showOutput.value = true;
  },
);

const title = computed(() => {
  const job = props.job;
  const done = job.verbs.filter(verbIsDone).length;
  switch (job.state) {
    case "Running": {
      const verb = job.verbs.find((v) => v.state === "Running");
      return verb ? `Installing ${verb.name}` : (job.activity ?? "Starting");
    }
    case "Succeeded":
      return `Installed ${done} component${done === 1 ? "" : "s"}`;
    case "Failed": {
      const failed = job.verbs.find((v) => v.state === "Failed");
      return failed ? `${failed.name} failed` : "Winetricks failed";
    }
    case "Cancelled":
      return "Cancelled";
  }
  return "";
});

const retryVerbs = computed(() =>
  props.job.verbs
    .filter((v) => v.state === "Failed" || v.state === "NotRun")
    .map((v) => v.name),
);

function verbLabel(verb: WinetricksVerbProgress) {
  switch (verb.state) {
    case "Running":
      return (
        (verb.activity ?? "Installing") +
        (verb.percent !== null ? ` · ${verb.percent}%` : "")
      );
    case "Installed":
      return "Installed";
    case "AlreadyInstalled":
      return "Already installed";
    case "Failed":
      return "Failed";
    case "NotRun":
      return "Not installed";
    case "Waiting":
      return "Waiting";
  }
}

function verbTime(verb: WinetricksVerbProgress) {
  if (!verb.startedAt || verb.state === "AlreadyInstalled") return "";
  return formatDuration((verb.finishedAt ?? now.value) - verb.startedAt);
}

async function run(action: () => Promise<void>) {
  actionError.value = undefined;
  try {
    await action();
  } catch (error) {
    actionError.value = (error as unknown as string).toString();
  }
}

const openLog = () => run(() => openWinetricksLog(props.job.gameId));
const retry = () =>
  run(() => installWinetricks(props.job.gameId, retryVerbs.value));
async function cancel() {
  cancelling.value = true;
  await run(() => cancelWinetricks(props.job.gameId));
}
</script>

<style scoped>
.winetricks-indeterminate {
  animation: winetricks-slide 1.6s ease-in-out infinite;
}
@keyframes winetricks-slide {
  from {
    transform: translateX(-100%);
  }
  to {
    transform: translateX(250%);
  }
}
</style>
