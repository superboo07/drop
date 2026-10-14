<template>
  <div class="space-y-8">
    <WinetricksProgress v-if="job" :job="job" />

    <div
      v-if="protonDefaults"
      class="flex flex-wrap items-center justify-between gap-2 rounded-md bg-blue-900/20 px-3 py-2 outline outline-1 outline-blue-500/30"
    >
      <p class="text-sm text-zinc-300">
        Your server recommends Proton settings for this version.
      </p>
      <button
        type="button"
        class="text-sm font-medium text-blue-400 hover:text-blue-300"
        @click="() => resetAll()"
      >
        Reset all to recommended
      </button>
    </div>

    <div>
      <h3 class="text-sm font-medium leading-6 text-zinc-100">
        Compatibility
      </h3>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        Toggle common Proton compatibility workarounds for this game
      </p>

      <div class="mt-3 space-y-3">
        <div
          v-for="toggle in compatToggles"
          :key="toggle.key"
          class="flex flex-row items-center justify-between"
        >
          <div>
            <h4 class="text-sm font-medium leading-6 text-zinc-100">
              {{ toggle.label }}
              <RecommendedBadge :state="toggleState(toggle.key)" />
            </h4>
            <p class="mt-1 text-sm leading-6 text-zinc-400">
              {{ toggle.description }}
            </p>
          </div>
          <Switch
            v-model="model[toggle.key]"
            :class="[
              model[toggle.key] ? 'bg-blue-600' : 'bg-zinc-700',
              'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out',
            ]"
          >
            <span
              :class="[
                model[toggle.key] ? 'translate-x-5' : 'translate-x-0',
                'pointer-events-none relative inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out',
              ]"
            />
          </Switch>
        </div>
      </div>
    </div>

    <div>
      <h3 class="text-sm font-medium leading-6 text-zinc-100">
        Extra environment variables
        <RecommendedBadge :state="envState" />
      </h3>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        One <code>KEY=value</code> pair per line, passed to Proton/Wine when
        launching this game
      </p>
      <textarea
        v-model="model.extraEnvVars"
        rows="3"
        placeholder="DXVK_HUD=fps&#10;WINEDLLOVERRIDES=dxgi=n"
        class="mt-3 block w-full rounded-md bg-white/5 p-3 font-mono text-sm text-white outline-1 -outline-offset-1 outline-white/10 focus:outline-2 focus:-outline-offset-2 focus:outline-blue-500"
      />
    </div>

    <div v-if="recommendedVerbs.length > 0">
      <h3 class="text-sm font-medium leading-6 text-zinc-100">
        Recommended components
      </h3>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        Your server suggests installing these into this game's Proton prefix
      </p>
      <div class="mt-3 flex flex-wrap items-center gap-2">
        <span
          v-for="verb in recommendedVerbs"
          :key="verb"
          class="inline-flex items-center gap-x-1.5 rounded-md bg-white/5 px-2 py-1 font-mono text-xs text-zinc-200 outline-1 -outline-offset-1 outline-white/10"
        >
          {{ verb }}
          <span
            v-if="chipState(verb) === 'installed'"
            class="text-green-500"
            >✓ installed</span
          >
          <span
            v-else-if="chipState(verb) === 'installing'"
            class="text-blue-300"
            >installing…</span
          >
          <span v-else-if="chipState(verb) === 'queued'" class="text-zinc-400"
            >queued</span
          >
          <span v-else-if="chipState(verb) === 'failed'" class="text-red-400"
            >failed</span
          >
        </span>
        <button
          v-if="unqueuedMissingVerbs.length > 0"
          type="button"
          class="rounded-md bg-blue-600 px-2.5 py-1 text-xs font-semibold text-white hover:bg-blue-500"
          @click="() => installMissing()"
        >
          {{ running ? "Add missing to queue" : "Install missing" }}
        </button>
      </div>
      <p v-if="recommendedError" class="mt-2 text-sm text-red-500">
        {{ recommendedError }}
      </p>
    </div>

    <div>
      <h3 class="text-sm font-medium leading-6 text-zinc-100">Winetricks</h3>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        Install a component (DirectX, .NET, fonts, etc.) into this game's
        Proton prefix
      </p>

      <div class="relative mt-3">
        <MagnifyingGlassIcon
          class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-zinc-500"
        />
        <input
          v-model="query"
          type="text"
          placeholder="Search verbs (e.g. dotnet48, vcrun2019, corefonts)"
          class="block w-full rounded-md bg-white/5 py-2 pl-9 pr-3 text-sm text-white outline-1 -outline-offset-1 outline-white/10 focus:outline-2 focus:-outline-offset-2 focus:outline-blue-500"
        />
      </div>

      <p v-if="verbsError" class="mt-2 text-sm text-red-500">
        {{ verbsError }}
      </p>

      <p
        v-else-if="verbsLoading && query.trim()"
        class="mt-2 text-sm text-zinc-400"
      >
        Loading winetricks verbs…
      </p>

      <ul
        v-else-if="query.trim()"
        class="mt-3 max-h-64 divide-y divide-zinc-800 overflow-y-auto rounded-md bg-white/5"
      >
        <li v-for="verb in filteredVerbs" :key="verb.name">
          <button
            @click="() => install(verb.name)"
            type="button"
            :disabled="inJob(verb.name)"
            class="flex w-full items-center justify-between gap-3 px-3 py-2 text-left hover:bg-zinc-800 disabled:hover:bg-transparent"
          >
            <span class="flex min-w-0 flex-col items-start">
              <span class="text-sm font-medium text-zinc-100">{{
                verb.name
              }}</span>
              <span class="text-xs text-zinc-400">{{ verb.description }}</span>
            </span>
            <span
              :class="[
                'shrink-0 text-xs font-semibold',
                inJob(verb.name) ? 'text-zinc-500' : 'text-blue-400',
              ]"
              >{{
                inJob(verb.name)
                  ? "In queue"
                  : running
                    ? "Add to queue"
                    : "Install"
              }}</span
            >
          </button>
        </li>
        <li
          v-if="filteredVerbs.length === 0"
          class="px-3 py-2 text-sm text-zinc-400"
        >
          No matching verbs
        </li>
      </ul>

      <p v-if="running" class="mt-2 text-xs text-zinc-400">
        Verbs you pick now are installed once the current ones are done.
      </p>
      <p v-if="installError" class="mt-2 text-sm text-red-500">
        {{ installError }}
      </p>
    </div>

    <div>
      <h3 class="text-sm font-medium leading-6 text-zinc-100">Winecfg</h3>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        Open Wine's configuration tool for this game's Proton prefix
      </p>
      <button
        @click="() => runWinecfg()"
        type="button"
        :disabled="running"
        class="mt-3 inline-flex items-center gap-x-2 rounded-md bg-blue-600 px-3.5 py-2.5 text-sm font-semibold text-white shadow-sm hover:bg-blue-500 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 disabled:bg-zinc-800 disabled:text-zinc-400"
      >
        <Cog6ToothIcon class="h-5 w-5" aria-hidden="true" />
        Run Winecfg
      </button>
      <p v-if="running" class="mt-2 text-xs text-zinc-400">
        Available once winetricks is done with the prefix.
      </p>
      <p v-if="winecfgError" class="mt-2 text-sm text-red-500">
        {{ winecfgError }}
      </p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Cog6ToothIcon, MagnifyingGlassIcon } from "@heroicons/vue/24/outline";
import { Switch } from "@headlessui/vue";
import type { GameVersion } from "~/types";
import RecommendedBadge from "./RecommendedBadge.vue";
import WinetricksProgress from "../WinetricksProgress.vue";
import {
  PROTON_DEFAULTS_KEY,
  disabledFromRecommendation,
  recommendationState,
  resetToRecommended,
} from "~/composables/proton-defaults";

const props = defineProps<{
  gameId: string;
}>();

const model = defineModel<GameVersion["userConfiguration"]>({ required: true });

const compatToggles: Array<{
  key: "disableDxvk" | "disableEsync" | "disableFsync";
  label: string;
  description: string;
}> = [
  {
    key: "disableDxvk",
    label: "Disable DXVK",
    description: "Use Wine's built-in WineD3D instead of DXVK for Direct3D",
  },
  {
    key: "disableEsync",
    label: "Disable Esync",
    description: "Turn off Wine's eventfd-based sync primitives",
  },
  {
    key: "disableFsync",
    label: "Disable Fsync",
    description: "Turn off Wine's futex-based sync primitives",
  },
];

type WinetricksVerb = {
  category: string;
  name: string;
  description: string;
};

const verbs = ref<WinetricksVerb[]>([]);
const verbsError = ref<string | undefined>();
const verbsLoading = ref(true);
const query = ref("");

const filteredVerbs = computed(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return [];
  return verbs.value
    .filter(
      (verb) =>
        verb.name.toLowerCase().includes(q) ||
        verb.description.toLowerCase().includes(q)
    )
    .slice(0, 50);
});

// Runs in the background on the Rust side and is cached there, but the
// first `winetricks list-all` of a session can take minutes on some systems.
invokeWithTimeout<WinetricksVerb[]>("list_winetricks_verbs", undefined, 300_000)
  .then((result) => (verbs.value = result))
  .catch((error) => {
    verbsError.value = (error as unknown as string).toString();
  })
  .finally(() => (verbsLoading.value = false));

const job = useWinetricksJob(() => props.gameId);
const running = computed(() => job.value?.state === "Running");

// Its result is shown right here, so it doesn't need a toast too
const viewing = useWinetricksViewing();
onMounted(() => (viewing.value = props.gameId));
onUnmounted(() => {
  if (viewing.value === props.gameId) viewing.value = undefined;
});

// Verbs the running job will still install
function inJob(verb: string) {
  return (
    running.value &&
    !!job.value?.verbs.some(
      (v) => v.name === verb && (v.state === "Waiting" || v.state === "Running"),
    )
  );
}

const installError = ref<string | undefined>();

async function install(verb: string) {
  installError.value = undefined;
  try {
    await installWinetricks(props.gameId, [verb]);
  } catch (error) {
    installError.value = (error as unknown as string).toString();
  }
}

const protonDefaults = inject(PROTON_DEFAULTS_KEY, null);

const onOff = (disabled: boolean) => (disabled ? "off" : "on");

function toggleState(key: "disableDxvk" | "disableEsync" | "disableFsync") {
  const recommended = {
    disableDxvk: protonDefaults?.dxvk,
    disableEsync: protonDefaults?.esync,
    disableFsync: protonDefaults?.fsync,
  }[key];
  return recommendationState(
    disabledFromRecommendation(recommended ?? null),
    model.value[key],
    onOff,
  );
}

const envState = computed(() =>
  recommendationState(
    protonDefaults?.extraEnvVars,
    model.value.extraEnvVars,
    () => "different variables",
  ),
);

async function resetAll() {
  if (protonDefaults) await resetToRecommended(model.value, protonDefaults);
}

const recommendedVerbs = protonDefaults?.winetricks ?? [];
const installedVerbs = ref<string[]>([]);
const missingVerbs = computed(() =>
  recommendedVerbs.filter((v) => !installedVerbs.value.includes(v)),
);
const unqueuedMissingVerbs = computed(() =>
  missingVerbs.value.filter((v) => !inJob(v)),
);
const recommendedError = ref<string | undefined>();

function chipState(verb: string) {
  const progress = job.value?.verbs.find((v) => v.name === verb);
  if (progress?.state === "Running") return "installing";
  if (progress?.state === "Waiting") return "queued";
  if (installedVerbs.value.includes(verb) || (progress && verbIsDone(progress)))
    return "installed";
  if (progress?.state === "Failed") return "failed";
  return undefined;
}

function refreshInstalled() {
  if (recommendedVerbs.length === 0) return;
  invokeWithTimeout<string[]>("fetch_installed_winetricks", {
    gameId: props.gameId,
  })
    .then((result) => (installedVerbs.value = result))
    .catch(() => {
      // Only used to mark installed verbs; installing works without it.
    });
}
refreshInstalled();
watch(running, (now, before) => {
  if (before && !now) refreshInstalled();
});

async function installMissing() {
  recommendedError.value = undefined;
  try {
    await installWinetricks(props.gameId, unqueuedMissingVerbs.value);
  } catch (error) {
    recommendedError.value = (error as unknown as string).toString();
  }
}

const winecfgError = ref<string | undefined>();

async function runWinecfg() {
  winecfgError.value = undefined;
  try {
    await invokeWithTimeout("run_winecfg", { gameId: props.gameId });
  } catch (error) {
    winecfgError.value = (error as unknown as string).toString();
  }
}
</script>
