<template>
  <div class="space-y-8">
    <div>
      <h3 class="text-sm font-medium leading-6 text-zinc-100">
        LunaTranslator
      </h3>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        Launch this game with LunaTranslator attached, hooking its text out of
        the Proton prefix as you play
      </p>

      <p
        v-if="lunaStatus && !lunaStatus.configured"
        class="mt-3 rounded-md bg-zinc-500/10 p-3 text-sm text-zinc-400 outline outline-zinc-500/20"
      >
        No LunaTranslator is set up yet. Add one under
        <span class="font-semibold text-zinc-200">Settings → Translation</span>
        before turning this on.
      </p>

      <div class="mt-3 space-y-3">
        <div
          v-for="toggle in lunaToggles"
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
            :disabled="toggle.disabled"
            :class="[
              model[toggle.key] ? 'bg-blue-600' : 'bg-zinc-700',
              toggle.disabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer',
              'relative inline-flex h-6 w-11 flex-shrink-0 rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out',
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
  </div>
</template>

<script setup lang="ts">
import { Switch } from "@headlessui/vue";
import type { GameVersion } from "~/types";

const model = defineModel<GameVersion["userConfiguration"]>({ required: true });

type LunaStatus = {
  supported: boolean;
  configured: boolean;
  bridgeReady: boolean;
  bridgePath: string | undefined;
};

const lunaStatus = ref<LunaStatus | undefined>();
invokeWithTimeout<LunaStatus>("fetch_luna_status")
  .then((result) => (lunaStatus.value = result))
  .catch(() => {
    // Status is only used to soften the UI when LunaTranslator isn't set up;
    // the launch itself reports a real error if it isn't. Nothing to recover.
  });

const lunaToggles = computed<
  Array<{
    key: "lunaTranslator" | "nestedSession";
    label: string;
    description: string;
    disabled: boolean;
  }>
>(() => [
  {
    key: "lunaTranslator",
    label: "Launch with LunaTranslator",
    description: "Start LunaTranslator with the game and hook into it",
    disabled: !lunaStatus.value?.configured,
  },
  {
    key: "nestedSession",
    label: "Nested window session",
    description:
      "Run the game and LunaTranslator as draggable windows inside one session — needed in the Steam Deck's game mode, where only one window is ever shown",
    disabled: !model.value.lunaTranslator,
  },
]);
</script>
