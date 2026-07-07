<template>
  <div class="space-y-8">
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

invokeWithTimeout<WinetricksVerb[]>("list_winetricks_verbs")
  .then((result) => (verbs.value = result))
  .catch((error) => {
    verbsError.value = (error as unknown as string).toString();
  });

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
