<!-- eslint-disable vue/no-v-html -->
<template>
  <div v-if="game && unimportedVersions" class="px-4 sm:px-6 lg:px-8 py-8">
    <div class="sm:flex sm:items-center">
      <div class="sm:flex-auto">
        <h1 class="text-base font-semibold text-white">
          {{ $t("library.admin.version.title") }}
        </h1>
        <p class="mt-2 text-sm text-gray-300">
          {{ $t("library.admin.version.description") }}
        </p>
      </div>
      <div class="mt-4 sm:mt-0 sm:ml-16 sm:flex-none">
        <NuxtLink
          :href="canImport ? `/admin/library/${game.id}/import` : ''"
          type="button"
          :class="[
            canImport ? 'bg-blue-600 hover:bg-blue-700' : 'bg-blue-800/50',
            'inline-flex w-fit items-center gap-x-2 rounded-md  px-3 py-1 text-sm font-semibold font-display text-white shadow-sm  focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600',
          ]"
        >
          {{
            canImport
              ? $t("library.admin.import.version.import")
              : $t("library.admin.import.version.noVersions")
          }}
        </NuxtLink>
      </div>
    </div>
    <div class="mt-8 flow-root">
      <div class="-mx-4 -my-2 overflow-x-auto sm:-mx-6 lg:-mx-8">
        <div class="inline-block min-w-full py-2 align-middle sm:px-6 lg:px-8">
          <table class="relative min-w-full divide-y divide-white/15">
            <thead>
              <tr>
                <th></th>
                <th
                  scope="col"
                  class="py-3 pr-3 pl-4 text-left text-xs font-medium tracking-wide text-gray-400 uppercase sm:pl-0"
                >
                  {{ $t("library.admin.version.table.name") }}
                </th>
                <th
                  scope="col"
                  class="px-3 py-3 text-left text-xs font-medium tracking-wide text-gray-400 uppercase"
                >
                  {{ $t("library.admin.version.table.path") }}
                </th>
                <th
                  scope="col"
                  class="px-3 py-3 text-left text-xs font-medium tracking-wide text-gray-400 uppercase"
                >
                  {{ $t("library.admin.version.table.delta") }}
                </th>
                <th
                  scope="col"
                  class="px-3 py-3 text-left text-xs font-medium tracking-wide text-gray-400 uppercase"
                >
                  {{ $t("library.admin.version.table.setup") }}
                </th>
                <th
                  scope="col"
                  class="px-3 py-3 text-left text-xs font-medium tracking-wide text-gray-400 uppercase"
                >
                  {{ $t("library.admin.version.table.launch") }}
                </th>
                <th scope="col" class="py-3 pr-4 pl-3 sm:pr-0">
                  <span class="sr-only">{{ $t("common.edit") }}</span>
                </th>
              </tr>
            </thead>
            <draggable
              :list="game.versions"
              handle=".handle"
              class="divide-y divide-white/10"
              tag="tbody"
              @update="() => updateVersionOrder()"
            >
              <template #item="{ element: version }: { element: VersionType }">
                <tr :key="version.versionId">
                  <td>
                    <Bars3Icon
                      class="cursor-move w-6 h-6 text-zinc-400 handle"
                    />
                  </td>
                  <td class="py-4 pr-3 pl-4 sm:pl-0">
                    <div class="flex flex-col">
                      <span
                        class="text-sm font-medium whitespace-nowrap text-white"
                        >{{ version.displayName ?? version.versionPath }}</span
                      >
                      <span class="text-xs text-zinc-500 mono">{{
                        version.versionId
                      }}</span>
                    </div>
                  </td>
                  <td class="px-3 py-4 text-sm whitespace-nowrap text-gray-400">
                    {{ version.versionPath }}
                  </td>
                  <td class="px-3 py-4 text-sm whitespace-nowrap text-gray-400">
                    {{ version.delta }}
                  </td>

                  <td class="px-3 py-4 text-sm whitespace-nowrap text-gray-400">
                    <ul class="space-y-2">
                      <GameEditorVersionConfig
                        v-for="config in version.setups"
                        :key="config.setupId"
                        :config="config"
                      />
                      <li
                        v-if="version.setups.length == 0"
                        class="text-xs uppercase font-display text-zinc-700 font-semibold"
                      >
                        {{ $t("library.admin.version.noSetups") }}
                      </li>
                    </ul>
                  </td>
                  <td class="px-3 py-4 text-sm whitespace-nowrap text-gray-400">
                    <div v-if="version.onlySetup">
                      {{ $t("library.admin.version.setupOnly") }}
                    </div>
                    <ul v-else class="space-y-2">
                      <GameEditorVersionConfig
                        v-for="config in version.launches"
                        :key="config.launchId"
                        :config="config"
                      />
                    </ul>
                  </td>
                  <td
                    class="py-4 pr-4 pl-3 text-right text-sm font-medium whitespace-nowrap sm:pr-0 space-x-2"
                  >
                    <button
                      class="text-blue-400 hover:text-blue-300"
                      @click="() => openEditModal(version)"
                    >
                      {{ $t("common.edit")
                      }}<span class="sr-only"
                        >,
                        {{ version.displayName ?? version.versionPath }}</span
                      >
                    </button>
                    <button
                      v-if="version.versionPath !== null"
                      class="text-amber-400 hover:text-amber-300"
                      @click="() => resyncVersion(version)"
                    >
                      {{ $t("library.admin.version.resync.action") }}
                    </button>
                    <button
                      v-else
                      class="text-amber-400 hover:text-amber-300 disabled:opacity-50"
                      :disabled="depotUploads.length === 0"
                      @click="() => openReplaceModal(version)"
                    >
                      {{ $t("library.admin.version.replace.action") }}
                    </button>
                    <button
                      class="text-red-400 hover:text-red-300"
                      @click="() => deleteVersion(version.versionId)"
                    >
                      {{ $t("common.delete") }}
                    </button>
                  </td>
                </tr></template
              >
            </draggable>
          </table>
        </div>
      </div>
    </div>
    <GameEditorVersionEditForm
      v-if="editingVersion"
      v-model="showEditModal"
      :game-id="game.id"
      :game-type="game.type"
      :version="editingVersion"
      @saved="refreshGame"
    />
    <ModalTemplate v-model="showReplaceModal">
      <template #default>
        <div v-if="replacingVersion" class="flex flex-col gap-y-4">
          <h1 class="text-lg font-semibold font-display text-zinc-100">
            {{ $t("library.admin.version.replace.title") }}
          </h1>
          <Listbox
            v-model="selectedUnimportedVersionId"
            as="div"
            :disabled="depotUploads.length === 0"
          >
            <ListboxLabel
              class="block text-sm font-medium leading-6 text-zinc-100"
              >{{ $t("library.admin.version.replace.picker") }}</ListboxLabel
            >
            <div class="relative mt-2">
              <ListboxButton
                class="relative w-full cursor-default rounded-md bg-zinc-950 py-1.5 pl-3 pr-10 text-left text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-800 focus:outline-none focus:ring-2 focus:ring-blue-600 sm:text-sm sm:leading-6"
              >
                <span v-if="selectedUnimportedVersion" class="block truncate">{{
                  selectedUnimportedVersion.name
                }}</span>
                <span v-else class="block truncate text-zinc-600">{{
                  $t("library.admin.version.replace.noneAvailable")
                }}</span>
                <span
                  class="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-2"
                >
                  <ChevronUpDownIcon
                    class="h-5 w-5 text-gray-400"
                    aria-hidden="true"
                  />
                </span>
              </ListboxButton>
              <ListboxOptions
                class="absolute z-10 mt-1 max-h-60 w-full overflow-auto rounded-md bg-zinc-900 py-1 text-base shadow-lg ring-1 ring-zinc-800 focus:outline-none sm:text-sm"
              >
                <ListboxOption
                  v-for="depotVersion in depotUploads"
                  :key="depotVersion.identifier"
                  v-slot="{ active, selected }"
                  as="template"
                  :value="depotVersion.identifier"
                >
                  <li
                    :class="[
                      active ? 'bg-blue-600 text-white' : 'text-zinc-100',
                      'relative cursor-default select-none py-2 pl-3 pr-9',
                    ]"
                  >
                    <span
                      :class="[
                        selected ? 'font-semibold' : 'font-normal',
                        'block truncate',
                      ]"
                      >{{ depotVersion.name }}</span
                    >
                    <span
                      v-if="selected"
                      :class="[
                        active ? 'text-white' : 'text-blue-600',
                        'absolute inset-y-0 right-0 flex items-center pr-4',
                      ]"
                    >
                      <CheckIcon class="h-5 w-5" aria-hidden="true" />
                    </span>
                  </li>
                </ListboxOption>
              </ListboxOptions>
            </div>
          </Listbox>
          <div v-if="replaceError" class="w-fit rounded-md bg-red-600/10 p-4">
            <div class="flex">
              <div class="flex-shrink-0">
                <XCircleIcon class="h-5 w-5 text-red-600" aria-hidden="true" />
              </div>
              <div class="ml-3">
                <h3 class="text-sm font-medium text-red-600">
                  {{ replaceError }}
                </h3>
              </div>
            </div>
          </div>
        </div>
      </template>
      <template #buttons>
        <LoadingButton
          type="button"
          :loading="replaceLoading"
          :disabled="!selectedUnimportedVersionId"
          class="inline-flex w-full shadow-sm sm:ml-3 sm:w-auto"
          @click="() => performReplace(false)"
        >
          {{ $t("library.admin.version.replace.action") }}
        </LoadingButton>
        <button
          type="button"
          class="mt-3 inline-flex w-full justify-center rounded-md bg-zinc-900 px-3 py-2 text-sm font-semibold text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-700 hover:bg-zinc-950 sm:mt-0 sm:w-auto"
          @click="showReplaceModal = false"
        >
          {{ $t("cancel") }}
        </button>
      </template>
    </ModalTemplate>
  </div>
  <div v-else class="grow w-full flex items-center justify-center">
    <div class="flex flex-col items-center">
      <ExclamationCircleIcon
        class="h-12 w-12 text-red-600"
        aria-hidden="true"
      />
      <div class="mt-3 text-center sm:mt-5">
        <h1 class="text-3xl font-semibold font-display leading-6 text-zinc-100">
          {{ $t("library.admin.offlineTitle") }}
        </h1>
        <div class="mt-4">
          <p class="text-sm text-zinc-400 max-w-md">
            {{ $t("library.admin.offline") }}
          </p>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import type { SerializeObject } from "nitropack";
import type { H3Error } from "h3";
import { ExclamationCircleIcon, Bars3Icon } from "@heroicons/vue/24/outline";
import { XCircleIcon } from "@heroicons/vue/16/solid";
import { CheckIcon, ChevronUpDownIcon } from "@heroicons/vue/20/solid";
import {
  Listbox,
  ListboxButton,
  ListboxLabel,
  ListboxOption,
  ListboxOptions,
} from "@headlessui/vue";
import type { AdminFetchGameType } from "~/server/api/v1/admin/game/[id]/index.get";
import type { UnimportedVersionInformation } from "~/server/internal/library";

const props = defineProps<{
  unimportedVersions: UnimportedVersionInformation[];
}>();

const { t } = useI18n();
const router = useRouter();

const hasDeleted = ref(false);

const canImport = computed(
  () => hasDeleted.value || props.unimportedVersions.length > 0,
);

const game = defineModel<SerializeObject<AdminFetchGameType>>({
  required: true,
});
if (!game.value)
  throw createError({
    statusCode: 500,
    statusMessage: "Game not provided to editor component",
  });

type VersionType = (typeof game.value.versions)[number];

async function updateVersionOrder() {
  try {
    const newVersionOrder = await $dropFetch(
      "/api/v1/admin/game/:id/versions",
      {
        method: "PATCH",
        body: {
          versions: game.value.versions.map((e) => e.versionId),
        },
        params: {
          id: game.value.id,
        },
      },
    );
    const newVersions = newVersionOrder.map(
      (id) => game.value.versions.find((k) => k.versionId == id)!,
    );
    game.value.versions = newVersions;
  } catch (e) {
    createModal(
      ModalType.Notification,
      {
        title: t("errors.version.order.title"),
        description: t("errors.version.order.desc", {
          error: (e as H3Error)?.statusMessage ?? t("errors.unknown"),
        }),
        buttonText: t("common.close"),
      },
      (e, c) => c(),
    );
  }
}

async function deleteVersion(versionId: string) {
  try {
    await $dropFetch("/api/v1/admin/game/:id/versions", {
      method: "DELETE",
      body: {
        version: versionId,
      },
      params: {
        id: game.value.id,
      },
    });
    game.value.versions.splice(
      game.value.versions.findIndex((e) => e.versionId === versionId),
      1,
    );
    hasDeleted.value = true;
  } catch (e) {
    createModal(
      ModalType.Notification,
      {
        title: t("errors.version.delete.title"),
        description: t("errors.version.delete.desc", {
          error: (e as H3Error)?.statusMessage ?? t("errors.unknown"),
        }),
        buttonText: t("common.close"),
      },
      (e, c) => c(),
    );
  }
}

// -- Edit config --

const showEditModal = ref(false);
const editingVersion = ref<VersionType | undefined>();

function openEditModal(version: VersionType) {
  editingVersion.value = version;
  showEditModal.value = true;
}

async function refreshGame() {
  const { game: refreshed } = await $dropFetch("/api/v1/admin/game/:id", {
    params: { id: game.value.id },
  });
  game.value = refreshed;
}

// -- Resync (local) / replace (depot) files --

function showResyncError(e: unknown) {
  createModal(
    ModalType.Notification,
    {
      title: t("errors.version.resync.title"),
      description: t("errors.version.resync.desc", {
        error: (e as H3Error)?.statusMessage ?? t("errors.unknown"),
      }),
      buttonText: t("common.close"),
    },
    (e, c) => c(),
  );
}

async function performResync(version: VersionType, force: boolean) {
  const { taskId } = await $dropFetch(
    "/api/v1/admin/game/:id/versions/resync",
    {
      method: "POST",
      params: { id: game.value.id },
      body: { versionId: version.versionId, force },
    },
  );
  router.push(`/admin/task/${taskId}`);
}

function resyncVersion(version: VersionType) {
  createModal(
    ModalType.Confirmation,
    {
      title: t("library.admin.version.resync.confirmTitle"),
      description: t("library.admin.version.resync.confirmDesc", [
        version.displayName ?? version.versionPath ?? "",
      ]),
      buttonText: t("library.admin.version.resync.action"),
    },
    async (event, close) => {
      if (event !== "confirm") return close();
      close();
      try {
        await performResync(version, false);
      } catch (e) {
        if ((e as H3Error)?.statusCode === 409) {
          createModal(
            ModalType.Confirmation,
            {
              title: t("library.admin.version.resync.forceTitle"),
              description: (e as H3Error)?.statusMessage ?? t("errors.unknown"),
              buttonText: t("library.admin.version.resync.forceAction"),
            },
            async (event2, close2) => {
              if (event2 !== "confirm") return close2();
              close2();
              try {
                await performResync(version, true);
              } catch (e2) {
                showResyncError(e2);
              }
            },
          );
        } else {
          showResyncError(e);
        }
      }
    },
  );
}

const depotUploads = computed(() =>
  props.unimportedVersions.filter((v) => v.type === "depot"),
);

const showReplaceModal = ref(false);
const replacingVersion = ref<VersionType | undefined>();
const selectedUnimportedVersionId = ref<string | undefined>();
const replaceLoading = ref(false);
const replaceError = ref<string | undefined>();

const selectedUnimportedVersion = computed(() =>
  depotUploads.value.find(
    (v) => v.identifier === selectedUnimportedVersionId.value,
  ),
);

function openReplaceModal(version: VersionType) {
  replacingVersion.value = version;
  selectedUnimportedVersionId.value = depotUploads.value[0]?.identifier;
  replaceError.value = undefined;
  showReplaceModal.value = true;
}

async function performReplace(force: boolean) {
  if (!replacingVersion.value || !selectedUnimportedVersionId.value) return;
  replaceLoading.value = true;
  replaceError.value = undefined;
  try {
    const { taskId } = await $dropFetch(
      "/api/v1/admin/game/:id/versions/depot-replace",
      {
        method: "POST",
        params: { id: game.value.id },
        body: {
          versionId: replacingVersion.value.versionId,
          unimportedVersionId: selectedUnimportedVersionId.value,
          force,
        },
      },
    );
    showReplaceModal.value = false;
    router.push(`/admin/task/${taskId}`);
  } catch (e) {
    if ((e as H3Error)?.statusCode === 409 && !force) {
      createModal(
        ModalType.Confirmation,
        {
          title: t("library.admin.version.resync.forceTitle"),
          description: (e as H3Error)?.statusMessage ?? t("errors.unknown"),
          buttonText: t("library.admin.version.resync.forceAction"),
        },
        async (event, close) => {
          if (event !== "confirm") return close();
          close();
          await performReplace(true);
        },
      );
    } else {
      replaceError.value = (e as H3Error)?.statusMessage ?? t("errors.unknown");
    }
  } finally {
    replaceLoading.value = false;
  }
}
</script>
