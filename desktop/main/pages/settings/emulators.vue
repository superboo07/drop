<template>
  <div class="border-b border-zinc-700 py-5">
    <h3 class="text-base font-semibold font-display leading-6 text-zinc-100">
      Emulators
    </h3>
  </div>

  <p class="mt-5 text-sm leading-6 text-zinc-400">
    Games that run through an emulator normally use a copy of that emulator Drop
    downloads from your server. Turn on bring-your-own-emulator mode to run them
    with an emulator you've already installed on this machine instead — Drop
    won't ask you to install the server's copy for any emulator you've set up
    below.
  </p>

  <div class="mt-6 flex flex-row items-center justify-between gap-x-6">
    <div>
      <h3 class="text-sm font-medium leading-6 text-zinc-100">
        Bring your own emulator
      </h3>
      <p class="mt-1 text-sm leading-6 text-zinc-400">
        Emulators without a local copy set up keep using the server's.
      </p>
    </div>
    <Switch
      v-model="byoEmulator"
      :class="[
        byoEmulator ? 'bg-blue-600' : 'bg-zinc-700',
        'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out',
      ]"
    >
      <span
        :class="[
          byoEmulator ? 'translate-x-5' : 'translate-x-0',
          'pointer-events-none relative inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out',
        ]"
      />
    </Switch>
  </div>

  <div class="mt-8 mb-8">
    <h3 class="text-sm font-medium leading-6 text-zinc-100">Your emulators</h3>
    <p class="mt-1 text-sm leading-6 text-zinc-400">
      Point each emulator at its executable on this machine<template
        v-if="flatpakSupported"
        >, or pick it from your installed Flatpaks</template
      >. If some games need a different version of an emulator, give that
      version its own copy under <em>Specific versions</em>, and choose where
      games that run through it are installed.
      <code>{rom}</code> in the arguments is replaced with the game's file, and
      <code>{args}</code> with the arguments from the game's launch option.
      Left out, the game's file is added at the end, followed by its
      arguments.
    </p>

    <p
      v-if="emulators.length === 0"
      class="mt-4 rounded-lg bg-zinc-950 p-4 text-sm italic text-zinc-400 ring-2 ring-zinc-800"
    >
      No emulators found. Emulators from your library, and ones used by games
      you've installed, show up here.
    </p>

    <ul v-else role="list" class="mt-4 space-y-4">
      <li
        v-for="emulator in emulators"
        :key="emulator.id"
        class="rounded-lg bg-zinc-950 p-4 ring-2 ring-zinc-800"
      >
        <div class="flex flex-row items-center gap-x-3">
          <img
            v-if="emulator.iconObjectId"
            :src="useObject(emulator.iconObjectId)"
            class="size-8 rounded-sm"
          />
          <CpuChipIcon v-else class="size-8 text-zinc-500" />
          <div class="grow">
            <h4 class="text-sm font-semibold text-zinc-100">
              {{ emulator.name }}
            </h4>
            <p class="text-xs font-medium">
              <span v-if="hasLocalCopy(emulator.id)" class="text-green-400">{{
                summary(emulator.id)
              }}</span>
              <span v-else class="font-normal text-zinc-400"
                >Using the server's copy</span
              >
              <template v-if="overrides[emulator.id]?.installDir">
                <span class="text-zinc-600"> · </span>
                <span
                  v-if="installDirMissing(emulator.id)"
                  class="text-yellow-400"
                  >Install folder missing</span
                >
                <span v-else class="text-zinc-300"
                  >games install to
                  {{ baseName(overrides[emulator.id]!.installDir!) }}</span
                >
              </template>
            </p>
          </div>
        </div>

        <div class="mt-4">
          <h5
            class="text-xs font-semibold uppercase tracking-wide text-zinc-400"
          >
            Install games to
          </h5>
          <p class="mt-1 text-xs text-zinc-500">
            Where games that run through {{ emulator.name }} are installed by
            default. You can still change it when you install.
          </p>
          <EmulatorInstallDirSelector
            class="mt-2 max-w-xl"
            :install-dirs="installDirs"
            :model-value="overrides[emulator.id]?.installDir ?? null"
            @update:model-value="(value) => setInstallDir(emulator, value)"
            @add="() => addInstallDir(emulator)"
          />
          <p
            v-if="installDirMissing(emulator.id)"
            class="mt-2 text-xs text-yellow-400"
          >
            This folder isn't one of your install directories any more, so
            games start on the first one. Pick another folder, or add it back
            in Downloads.
          </p>
        </div>

        <div :class="['mt-4 transition', byoEmulator ? '' : 'opacity-60']">
          <h5
            class="text-xs font-semibold uppercase tracking-wide text-zinc-400"
          >
            All versions
          </h5>
          <LocalEmulatorEditor
            class="mt-2"
            :model-value="overrides[emulator.id]?.default ?? undefined"
            @update:model-value="(value) => setDefault(emulator, value)"
            empty-text="No local copy selected — the server's copy is used."
          />
        </div>

        <div
          v-if="emulator.versions.length > 0"
          :class="['mt-4 transition', byoEmulator ? '' : 'opacity-60']"
        >
          <button
            type="button"
            @click="() => toggleVersions(emulator.id)"
            class="flex flex-row items-center gap-x-1 text-xs font-semibold uppercase tracking-wide text-zinc-400 hover:text-zinc-200"
          >
            <ChevronRightIcon
              :class="[
                'size-4 transition',
                versionsOpen[emulator.id] ? 'rotate-90' : '',
              ]"
            />
            Specific versions
            <span
              v-if="versionOverrideCount(emulator.id) > 0"
              class="ml-1 rounded bg-green-500/15 px-1.5 py-0.5 normal-case tracking-normal text-green-300"
              >{{ versionOverrideCount(emulator.id) }} set</span
            >
          </button>

          <ul
            v-if="versionsOpen[emulator.id]"
            role="list"
            class="mt-3 space-y-3 border-l-2 border-zinc-800 pl-4"
          >
            <li v-for="version in emulator.versions" :key="version.versionId">
              <div class="flex flex-row flex-wrap items-baseline gap-x-2">
                <span class="text-sm font-medium text-zinc-100">{{
                  version.label
                }}</span>
                <span
                  v-if="version.usedLocally"
                  class="rounded bg-zinc-800 px-1.5 py-0.5 text-xs text-zinc-300"
                  >Used by your installed games</span
                >
              </div>
              <LocalEmulatorEditor
                class="mt-2"
                :model-value="versionValue(emulator.id, version.versionId)"
                @update:model-value="
                  (value) => setVersion(emulator, version, value)
                "
                :empty-text="
                  overrides[emulator.id]?.default
                    ? 'Uses the copy set for all versions.'
                    : 'Uses the server\'s copy.'
                "
              />
            </li>
          </ul>
        </div>
      </li>
    </ul>

    <p v-if="saveError" class="mt-3 text-sm text-red-500">{{ saveError }}</p>
  </div>
</template>

<script setup lang="ts">
import { Switch } from "@headlessui/vue";
import { CpuChipIcon } from "@heroicons/vue/24/outline";
import { ChevronRightIcon } from "@heroicons/vue/20/solid";
import { open } from "@tauri-apps/plugin-dialog";
import { platform } from "@tauri-apps/plugin-os";
import type {
  Collection,
  EmulatorOverride,
  Game,
  LocalEmulator,
  Settings,
} from "~/types";

type EmulatorVersionEntry = {
  versionId: string;
  label: string;
  usedLocally: boolean;
};
type EmulatorEntry = {
  id: string;
  name: string;
  iconObjectId?: string;
  versions: EmulatorVersionEntry[];
};

const flatpakSupported = platform() === "linux";

const settings = await invokeWithTimeout<Settings>("fetch_settings");
const byoEmulator = ref(settings.byoEmulator);
const overrides = ref<{ [id: string]: EmulatorOverride }>({
  ...settings.emulatorOverrides,
});
const saveError = ref<string | undefined>();
const installDirs = ref<string[]>(
  await invokeWithTimeout<string[]>("fetch_download_dir_stats"),
);

watch(byoEmulator, async (value) => {
  await invokeWithTimeout("update_settings", {
    newSettings: { byoEmulator: value },
  });
});

async function save() {
  saveError.value = undefined;
  try {
    await invokeWithTimeout("update_settings", {
      newSettings: { emulatorOverrides: overrides.value },
    });
  } catch (error) {
    saveError.value = `Couldn't save: ${error}`;
  }
}

function overrideFor(emulator: EmulatorEntry): EmulatorOverride {
  overrides.value[emulator.id] ??= {
    name: emulator.name,
    default: null,
    versions: {},
    installDir: null,
  };
  return overrides.value[emulator.id]!;
}

// Drop entries with nothing set, so "no override" has one representation.
function prune(id: string) {
  const entry = overrides.value[id];
  if (
    entry &&
    !entry.default &&
    Object.keys(entry.versions).length === 0 &&
    !entry.installDir
  ) {
    delete overrides.value[id];
  }
}

async function setDefault(
  emulator: EmulatorEntry,
  value: LocalEmulator | undefined,
) {
  overrideFor(emulator).default = value ?? null;
  prune(emulator.id);
  await save();
}

async function setVersion(
  emulator: EmulatorEntry,
  version: EmulatorVersionEntry,
  value: LocalEmulator | undefined,
) {
  const entry = overrideFor(emulator);
  if (value) {
    entry.versions[version.versionId] = {
      ...value,
      versionName: version.label,
    };
  } else {
    delete entry.versions[version.versionId];
  }
  prune(emulator.id);
  await save();
}

async function setInstallDir(emulator: EmulatorEntry, value: string | null) {
  overrideFor(emulator).installDir = value;
  prune(emulator.id);
  await save();
}

async function addInstallDir(emulator: EmulatorEntry) {
  saveError.value = undefined;
  try {
    const dir = await open({ multiple: false, directory: true });
    if (!dir) return;
    await invokeWithTimeout("add_download_dir", { newDir: dir });
    installDirs.value = await invokeWithTimeout<string[]>(
      "fetch_download_dir_stats",
    );
    await setInstallDir(emulator, dir);
  } catch (error) {
    saveError.value = `Couldn't add that directory: ${error}`;
  }
}

function installDirMissing(id: string) {
  const dir = overrides.value[id]?.installDir;
  return !!dir && !installDirs.value.includes(dir);
}

function baseName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

function hasLocalCopy(id: string) {
  const entry = overrides.value[id];
  return !!entry && (!!entry.default || versionOverrideCount(id) > 0);
}

function versionValue(
  id: string,
  versionId: string,
): LocalEmulator | undefined {
  const entry = overrides.value[id]?.versions[versionId];
  if (!entry) return undefined;
  const { versionName: _, ...local } = entry;
  return local;
}

function versionOverrideCount(id: string) {
  return Object.keys(overrides.value[id]?.versions ?? {}).length;
}

function summary(id: string) {
  const entry = overrides.value[id];
  if (!entry) return "";
  const count = versionOverrideCount(id);
  const parts = [
    entry.default ? "Your own copy for all versions" : undefined,
    count > 0
      ? `your own copy for ${count} specific version${count === 1 ? "" : "s"}`
      : undefined,
  ].filter(Boolean);
  const text = parts.join(", ");
  const sentence = text.charAt(0).toUpperCase() + text.slice(1);
  return byoEmulator.value
    ? sentence
    : `${sentence} — turn on bring your own emulator to use it`;
}

const versionsOpen = ref<{ [id: string]: boolean }>(
  Object.fromEntries(
    Object.entries(overrides.value).map(([id, entry]) => [
      id,
      Object.keys(entry.versions).length > 0,
    ]),
  ),
);
function toggleVersions(id: string) {
  versionsOpen.value[id] = !versionsOpen.value[id];
}

// Emulators come from three places: ones in the library (or its
// collections), ones that installed games launch through (which may not be
// in the library at all), and ones already set up here (so an entry never
// disappears while it's still in effect, e.g. offline). Their versions come
// from the server where possible, plus whichever versions installed games
// use and ones already set up here.
async function loadEmulators(): Promise<EmulatorEntry[]> {
  const entries = new Map<string, EmulatorEntry>();
  const add = (id: string, name: string, iconObjectId?: string) => {
    if (!entries.has(id)) {
      entries.set(id, { id, name, iconObjectId, versions: [] });
    }
  };

  try {
    const library = await invokeWithTimeout<{
      library: Game[];
      collections: Collection[];
      other: Game[];
      missing: Game[];
    }>("fetch_library", {});
    const allGames = [
      ...library.library,
      ...library.collections.flatMap((c) => c.entries.map((e) => e.game)),
      ...library.other,
      ...library.missing,
    ];
    for (const game of allGames) {
      if (game.type === "Emulator") {
        add(game.id, game.mName, game.mIconObjectId);
      }
    }
  } catch (e) {
    console.error("failed to fetch library for emulators", e);
  }

  const referenced = await invokeWithTimeout<
    Array<{ gameId: string; versionId: string }>
  >("fetch_referenced_emulators");
  await Promise.allSettled(
    [...new Set(referenced.map((r) => r.gameId))]
      .filter((id) => !entries.has(id))
      .map(async (id) => {
        const { game } = await useGame(id);
        add(id, game.mName, game.mIconObjectId);
      }),
  );

  for (const [id, entry] of Object.entries(overrides.value)) {
    add(id, entry.name);
  }

  await Promise.allSettled(
    [...entries.values()].map(async (emulator) => {
      const versions = new Map<string, EmulatorVersionEntry>();
      try {
        const options = await invokeWithTimeout<VersionOption[]>(
          "fetch_game_version_options",
          { gameId: emulator.id },
        );
        for (const option of options) {
          versions.set(option.versionId, {
            versionId: option.versionId,
            label: option.displayName ?? option.versionPath,
            usedLocally: false,
          });
        }
      } catch (e) {
        console.error(`failed to fetch versions of ${emulator.id}`, e);
      }
      for (const [versionId, version] of Object.entries(
        overrides.value[emulator.id]?.versions ?? {},
      )) {
        if (!versions.has(versionId)) {
          versions.set(versionId, {
            versionId,
            label: version.versionName,
            usedLocally: false,
          });
        }
      }
      for (const { versionId } of referenced.filter(
        (r) => r.gameId === emulator.id,
      )) {
        const existing = versions.get(versionId);
        if (existing) existing.usedLocally = true;
        else {
          versions.set(versionId, {
            versionId,
            label: `Version ${versionId.slice(0, 8)}`,
            usedLocally: true,
          });
        }
      }
      emulator.versions = [...versions.values()];
    }),
  );

  return [...entries.values()].sort((a, b) => a.name.localeCompare(b.name));
}

const emulators = ref<EmulatorEntry[]>(await loadEmulators());
</script>
