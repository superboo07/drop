<template>
  <div class="border-b border-zinc-700 py-5">
    <h3 class="text-base font-semibold font-display leading-6 text-zinc-100">
      LunaTranslator
    </h3>
  </div>

  <p class="mt-5 text-sm leading-6 text-zinc-400">
    Drop can launch
    <a
      href="https://github.com/HIllya51/LunaTranslator"
      target="_blank"
      @click.prevent="
        openExternalLink('https://github.com/HIllya51/LunaTranslator')
      "
      class="text-blue-400 hover:text-blue-500"
      >LunaTranslator</a
    >
    alongside a Windows game and hook the game's text out of the Proton prefix
    for it, so untranslated games can be read as they're played. Point Drop at
    your own copy below, then turn it on per game from that game's Proton
    settings.
  </p>

  <div
    v-if="status && !status.supported"
    class="mt-4 rounded-md bg-yellow-500/10 p-4 outline outline-yellow-500/20"
  >
    <div class="flex">
      <div class="shrink-0">
        <ExclamationTriangleIcon
          class="size-5 text-yellow-400"
          aria-hidden="true"
        />
      </div>
      <div class="ml-3">
        <h3 class="text-sm font-medium text-yellow-200">Linux only</h3>
        <div class="mt-2 text-sm text-yellow-200/85">
          <p>
            The bridge exists to get text out of Wine, so this integration only
            does anything on Linux.
          </p>
        </div>
      </div>
    </div>
  </div>

  <template v-else>
    <!-- LunaTranslator itself -->
    <div class="mt-6">
      <h3 class="text-sm font-medium leading-6 text-zinc-100">
        LunaTranslator AppImage
      </h3>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        The AppImage Drop should run. An unpacked LunaTranslator directory
        works too.
      </p>

      <div
        class="mt-3 flex flex-row items-center gap-x-3 rounded-lg bg-zinc-950 p-3 text-sm ring-2 ring-zinc-800"
      >
        <span v-if="lunaPath" class="grow break-all text-zinc-100">{{
          lunaPath
        }}</span>
        <span v-else class="grow italic text-zinc-400">Nothing selected.</span>
        <button
          v-if="lunaPath"
          @click="() => setLunaPath(null)"
          type="button"
          class="shrink-0 text-red-400 hover:text-red-300"
        >
          Clear
        </button>
      </div>

      <div class="mt-3 flex flex-row gap-x-3">
        <button
          @click="pickLuna"
          type="button"
          class="inline-flex items-center gap-x-2 rounded-md bg-blue-600 px-3.5 py-2.5 text-sm font-semibold text-white shadow-sm hover:bg-blue-500"
        >
          Select AppImage
        </button>
        <button
          @click="pickLunaDirectory"
          type="button"
          class="inline-flex items-center gap-x-2 rounded-md bg-zinc-800 px-3.5 py-2.5 text-sm font-semibold text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-700 hover:bg-zinc-900"
        >
          Select directory
        </button>
      </div>
    </div>

    <!-- The in-prefix bridge -->
    <div class="mt-8">
      <h3 class="text-sm font-medium leading-6 text-zinc-100">Bridge</h3>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        <code>LunaCompanion.exe</code> runs inside the game's Proton prefix,
        injects LunaTranslator's hook into the game and streams the text back
        out. It ships inside the AppImage, so Drop extracts its own copy — you
        only need to set a path here if you have the bridge somewhere else.
      </p>

      <div
        v-if="status?.bridgeReady"
        class="mt-3 rounded-md bg-green-500/10 p-4 outline outline-green-500/20"
      >
        <div class="flex">
          <div class="shrink-0">
            <CheckCircleIcon class="size-5 text-green-400" aria-hidden="true" />
          </div>
          <div class="ml-3">
            <h3 class="text-sm font-medium text-green-200">Bridge ready</h3>
            <div class="mt-2 break-all text-sm text-green-200/85">
              {{ status.bridgePath }}
            </div>
          </div>
        </div>
      </div>
      <div
        v-else
        class="mt-3 rounded-md bg-zinc-500/10 p-4 outline outline-zinc-500/20"
      >
        <div class="flex">
          <div class="shrink-0">
            <XCircleIcon class="size-5 text-zinc-400" aria-hidden="true" />
          </div>
          <div class="ml-3">
            <h3 class="text-sm font-medium text-zinc-200">
              Bridge not extracted yet
            </h3>
            <div class="mt-2 text-sm text-zinc-400">
              It'll be extracted automatically the first time you launch a game
              with LunaTranslator enabled, or you can do it now.
            </div>
          </div>
        </div>
      </div>

      <div class="mt-3 flex flex-row items-center gap-x-3">
        <LoadingButton
          :loading="extracting"
          :disabled="!lunaPath"
          @click="extract"
        >
          Extract bridge now
        </LoadingButton>
        <button
          @click="pickBridge"
          type="button"
          class="inline-flex items-center gap-x-2 rounded-md bg-zinc-800 px-3.5 py-2.5 text-sm font-semibold text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-700 hover:bg-zinc-900"
        >
          Use a different bridge
        </button>
        <button
          v-if="bridgeOverride"
          @click="() => setBridgePath(null)"
          type="button"
          class="text-sm font-semibold text-red-400 hover:text-red-300"
        >
          Reset to bundled
        </button>
      </div>

      <p v-if="bridgeOverride" class="mt-2 break-all text-sm text-zinc-400">
        Override: <code>{{ bridgeOverride }}</code>
      </p>
      <p v-if="extractError" class="mt-2 text-sm text-red-500">
        {{ extractError }}
      </p>
    </div>

    <!-- Port -->
    <div class="mt-8 mb-8">
      <h3 class="text-sm font-medium leading-6 text-zinc-100">Bridge port</h3>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        The loopback port LunaTranslator listens on for the bridge. Only change
        this if you've changed it in LunaTranslator too.
      </p>
      <input
        v-model.number="port"
        @change="savePort"
        type="number"
        min="1"
        max="65535"
        class="mt-3 block w-40 rounded-md bg-white/5 p-2 font-mono text-sm text-white outline-1 -outline-offset-1 outline-white/10 focus:outline-2 focus:-outline-offset-2 focus:outline-blue-500"
      />
    </div>
  </template>
</template>

<script setup lang="ts">
import {
  CheckCircleIcon,
  ExclamationTriangleIcon,
  XCircleIcon,
} from "@heroicons/vue/16/solid";
import { open } from "@tauri-apps/plugin-dialog";
import type { Settings } from "~/types";

type LunaStatus = {
  supported: boolean;
  configured: boolean;
  bridgeReady: boolean;
  bridgePath: string | undefined;
};

const status = ref<LunaStatus | undefined>();
const lunaPath = ref<string | null>(null);
const bridgeOverride = ref<string | null>(null);
const port = ref(52300);

const extracting = ref(false);
const extractError = ref<string | undefined>();

async function refreshStatus() {
  status.value = await invokeWithTimeout<LunaStatus>("fetch_luna_status");
}

const settings = await invokeWithTimeout<Settings>("fetch_settings");
lunaPath.value = settings.lunaTranslatorPath ?? null;
bridgeOverride.value = settings.lunaBridgePath ?? null;
port.value = settings.lunaPort;
await refreshStatus();

async function setLunaPath(path: string | null) {
  lunaPath.value = path;
  await invokeWithTimeout("update_settings", {
    newSettings: { lunaTranslatorPath: path },
  });
  await refreshStatus();
}

async function setBridgePath(path: string | null) {
  bridgeOverride.value = path;
  await invokeWithTimeout("update_settings", {
    newSettings: { lunaBridgePath: path },
  });
  await refreshStatus();
}

async function pickLuna() {
  const file = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "LunaTranslator", extensions: ["AppImage", "appimage"] }],
  });
  if (file) await setLunaPath(file);
}

async function pickLunaDirectory() {
  const directory = await open({ multiple: false, directory: true });
  if (directory) await setLunaPath(directory);
}

async function pickBridge() {
  const file = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "LunaCompanion", extensions: ["exe"] }],
  });
  if (file) await setBridgePath(file);
}

async function extract() {
  extracting.value = true;
  extractError.value = undefined;
  try {
    await invokeWithTimeout("extract_luna_bridge");
  } catch (error) {
    extractError.value = (error as unknown as string).toString();
  } finally {
    extracting.value = false;
    await refreshStatus();
  }
}

async function savePort() {
  if (!Number.isInteger(port.value) || port.value < 1 || port.value > 65535) {
    port.value = settings.lunaPort;
    return;
  }
  await invokeWithTimeout("update_settings", {
    newSettings: { lunaPort: port.value },
  });
}
</script>
