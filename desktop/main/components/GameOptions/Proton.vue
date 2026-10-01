<template>
  <div class="space-y-8">
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
          <span v-if="installedVerbs.includes(verb)" class="text-green-500"
            >✓ installed</span
          >
        </span>
        <button
          v-if="missingVerbs.length > 0"
          type="button"
          class="rounded-md bg-blue-600 px-2.5 py-1 text-xs font-semibold text-white hover:bg-blue-500"
          @click="() => installMissing()"
        >
          Install missing
        </button>
      </div>
      <p
        v-if="recommendedStatus"
        class="mt-2 text-sm"
        :class="recommendedError ? 'text-red-500' : 'text-green-500'"
      >
        {{ recommendedStatus }}
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
            class="flex w-full flex-col items-start px-3 py-2 text-left hover:bg-zinc-800"
          >
            <span class="text-sm font-medium text-zinc-100">{{
              verb.name
            }}</span>
            <span class="text-xs text-zinc-400">{{ verb.description }}</span>
          </button>
        </li>
        <li
          v-if="filteredVerbs.length === 0"
          class="px-3 py-2 text-sm text-zinc-400"
        >
          No matching verbs
        </li>
      </ul>

      <p
        v-if="installStatus"
        class="mt-2 text-sm"
        :class="installError ? 'text-red-500' : 'text-green-500'"
      >
        {{ installStatus }}
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
        class="mt-3 inline-flex items-center gap-x-2 rounded-md bg-blue-600 px-3.5 py-2.5 text-sm font-semibold text-white shadow-sm hover:bg-blue-500 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600"
      >
        <Cog6ToothIcon class="h-5 w-5" aria-hidden="true" />
        Run Winecfg
      </button>
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

const installStatus = ref<string | undefined>();
const installError = ref(false);

async function install(verb: string) {
  installError.value = false;
  installStatus.value = `Starting install of "${verb}"...`;
  try {
    await invokeWithTimeout("install_winetricks_verb", {
      gameId: props.gameId,
      verb,
    });
    installStatus.value = `Started installing "${verb}". Check the game's logs if it doesn't seem to do anything.`;
  } catch (error) {
    installError.value = true;
    installStatus.value = (error as unknown as string).toString();
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
const recommendedStatus = ref<string | undefined>();
const recommendedError = ref(false);

if (recommendedVerbs.length > 0)
  invokeWithTimeout<string[]>("fetch_installed_winetricks", {
    gameId: props.gameId,
  })
    .then((result) => (installedVerbs.value = result))
    .catch(() => {
      // Only used to mark installed verbs; installing works without it.
    });

async function installMissing() {
  recommendedError.value = false;
  const verbs = missingVerbs.value;
  try {
    await invokeWithTimeout("install_winetricks_verbs", {
      gameId: props.gameId,
      verbs,
    });
    recommendedStatus.value = `Started installing ${verbs.join(", ")}. Check the game's logs if it doesn't seem to do anything.`;
  } catch (error) {
    recommendedError.value = true;
    recommendedStatus.value = (error as unknown as string).toString();
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
