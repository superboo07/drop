<template>
  <div class="space-y-8">
    <div>
      <h3 class="text-sm font-medium leading-6 text-zinc-100">
        Install directory
      </h3>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        Open the folder this game is installed in
      </p>
      <button
        @click="() => openInstallDir()"
        type="button"
        class="mt-3 inline-flex items-center gap-x-2 rounded-md bg-blue-600 px-3.5 py-2.5 text-sm font-semibold text-white shadow-sm hover:bg-blue-500 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600"
      >
        <FolderIcon class="h-5 w-5" aria-hidden="true" />
        Open Install Directory
      </button>
      <p v-if="installDirError" class="mt-2 text-sm text-red-500">
        {{ installDirError }}
      </p>
    </div>

    <div v-if="protonEnabled">
      <h3 class="text-sm font-medium leading-6 text-zinc-100">
        Proton prefix
      </h3>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        Open this game's Wine/Proton prefix directory
      </p>
      <button
        @click="() => openWinePrefix()"
        type="button"
        class="mt-3 inline-flex items-center gap-x-2 rounded-md bg-blue-600 px-3.5 py-2.5 text-sm font-semibold text-white shadow-sm hover:bg-blue-500 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600"
      >
        <FolderIcon class="h-5 w-5" aria-hidden="true" />
        Open Wine Prefix
      </button>
      <p v-if="winePrefixError" class="mt-2 text-sm text-red-500">
        {{ winePrefixError }}
      </p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { FolderIcon } from "@heroicons/vue/24/outline";

const props = defineProps<{
  gameId: string;
  protonEnabled: boolean;
}>();

const installDirError = ref<string | undefined>();
const winePrefixError = ref<string | undefined>();

async function openInstallDir() {
  installDirError.value = undefined;
  try {
    await invokeWithTimeout("open_game_install_dir", { gameId: props.gameId });
  } catch (error) {
    installDirError.value = (error as unknown as string).toString();
  }
}

async function openWinePrefix() {
  winePrefixError.value = undefined;
  try {
    await invokeWithTimeout("open_game_wine_prefix", { gameId: props.gameId });
  } catch (error) {
    winePrefixError.value = (error as unknown as string).toString();
  }
}
</script>
