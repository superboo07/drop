<template>
  <ModalTemplate v-model="open" size-class="sm:max-w-2xl">
    <template #default>
      <div class="flex flex-col gap-y-4">
        <h1 class="text-lg font-semibold font-display text-zinc-100">
          {{ $t("library.admin.version.edit.title") }}
        </h1>

        <div>
          <label
            for="version-display-name"
            class="block text-sm font-medium leading-6 text-zinc-100"
          >
            {{ $t("library.admin.import.version.displayName") }}
          </label>
          <input
            id="version-display-name"
            v-model="form.displayName"
            type="text"
            class="mt-2 block w-full rounded-md border-radius-md bg-zinc-950 px-3 py-1.5 text-white outline-1 -outline-offset-1 outline-zinc-800 placeholder:text-zinc-500 focus:outline-1 focus:-outline-offset-1 focus:outline-blue-500 sm:text-sm/6"
            :placeholder="version.versionPath ?? ''"
          />
        </div>

        <!-- setup executable -->
        <div class="bg-zinc-800 p-4 rounded-xl flex flex-col gap-y-2">
          <div>
            <label class="block text-sm font-medium leading-6 text-zinc-100">{{
              $t("library.admin.import.version.setupCmd")
            }}</label>
            <p class="text-zinc-400 text-xs">
              {{ $t("library.admin.import.version.setupDesc") }}
            </p>
          </div>
          <ol v-if="form.setups.length > 0" class="divide-y-1 divide-zinc-700">
            <li
              v-for="(_setup, setupIdx) in form.setups"
              :key="setupIdx"
              class="py-2 inline-flex items-start gap-x-1 w-full"
            >
              <ImportVersionLaunchRow
                v-model="form.setups[setupIdx]"
                :version-guesses="undefined"
                :needs-name="false"
              />
              <button
                class="transition rounded p-1 bg-zinc-900/30 group hover:bg-red-600/30"
                @click="() => form.setups.splice(setupIdx, 1)"
              >
                <TrashIcon
                  class="transition size-5 text-zinc-700 group-hover:text-red-700"
                />
              </button>
            </li>
          </ol>
          <span
            v-else
            class="text-sm text-zinc-700 uppercase font-display font-bold"
            >{{ $t("library.admin.import.version.noSetups") }}</span
          >
          <LoadingButton
            :loading="false"
            class="w-fit"
            @click="() => form.setups.push({} as unknown as FormSetup)"
            >{{ $t("common.add") }}</LoadingButton
          >
        </div>

        <!-- setup mode -->
        <div class="relative">
          <SwitchGroup
            as="div"
            class="bg-zinc-800 p-4 rounded-xl flex items-center justify-between gap-4"
          >
            <span class="flex flex-grow flex-col">
              <SwitchLabel
                as="span"
                class="text-sm font-medium leading-6 text-zinc-100"
                passive
                >{{ $t("library.admin.import.version.setupMode") }}</SwitchLabel
              >
              <SwitchDescription as="span" class="text-sm text-zinc-400">{{
                $t("library.admin.import.version.setupModeDesc")
              }}</SwitchDescription>
            </span>
            <Switch
              v-model="form.onlySetup"
              :class="[
                form.onlySetup ? 'bg-blue-600' : 'bg-zinc-900',
                'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-blue-600 focus:ring-offset-2',
              ]"
            >
              <span
                aria-hidden="true"
                :class="[
                  form.onlySetup ? 'translate-x-5' : 'translate-x-0',
                  'pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out',
                ]"
              />
            </Switch>
          </SwitchGroup>
          <div
            v-if="gameType === GameType.Dependency"
            class="absolute inset-0 bg-zinc-900/50"
          />
        </div>

        <!-- launch executables -->
        <div class="relative flex flex-col gap-y-2 bg-zinc-800 p-4 rounded-xl">
          <div>
            <label class="block text-sm font-medium leading-6 text-zinc-100">{{
              $t("library.admin.import.version.launchCmd")
            }}</label>
            <p class="text-zinc-400 text-xs">
              {{ $t("library.admin.import.version.launchDesc") }}
            </p>
          </div>
          <ol
            v-if="form.launches.length > 0"
            class="divide-y-1 divide-zinc-700"
          >
            <li
              v-for="(launch, launchIdx) in form.launches"
              :key="launchIdx"
              class="py-2 inline-flex items-start gap-x-1 w-full"
            >
              <Disclosure
                v-slot="{ open: discOpen }"
                :default-open="true"
                as="div"
                class="py-2 px-3 w-full bg-zinc-900 rounded-lg"
              >
                <dt>
                  <DisclosureButton
                    class="flex w-full items-center text-left text-white"
                  >
                    <span v-if="launch.name" class="text-sm font-semibold">{{
                      launch.name
                    }}</span>
                    <span v-else class="text-sm text-zinc-500 italic">{{
                      $t("library.admin.import.version.noNameProvided")
                    }}</span>
                    <span class="ml-auto flex h-7 items-center">
                      <PlusIcon
                        v-if="!discOpen"
                        class="size-6"
                        aria-hidden="true"
                      />
                      <MinusIcon v-else class="size-6" aria-hidden="true" />
                    </span>
                    <button
                      class="ml-1 transition rounded p-1 bg-zinc-900/30 group hover:bg-red-600/30"
                      @click.prevent="() => form.launches.splice(launchIdx, 1)"
                    >
                      <TrashIcon
                        class="transition size-5 text-zinc-700 group-hover:text-red-700"
                      />
                    </button>
                  </DisclosureButton>
                </dt>
                <DisclosurePanel as="dd" class="mt-2">
                  <ImportVersionLaunchRow
                    v-model="form.launches[launchIdx]"
                    :version-guesses="undefined"
                    :needs-name="true"
                    :allow-emulator="true"
                    :type="gameType"
                  />
                </DisclosurePanel>
              </Disclosure>
            </li>
          </ol>
          <span
            v-else
            class="text-sm text-zinc-700 uppercase font-display font-bold"
            >{{ $t("library.admin.import.version.noLaunches") }}</span
          >
          <LoadingButton
            :loading="false"
            class="w-fit"
            @click="() => form.launches.push({} as unknown as FormLaunch)"
            >{{ $t("common.add") }}</LoadingButton
          >

          <div v-if="form.onlySetup" class="absolute inset-0 bg-zinc-900/50" />
        </div>

        <SwitchGroup
          as="div"
          class="bg-zinc-800 p-4 rounded-xl flex items-center gap-4 justify-between"
        >
          <span class="flex flex-grow flex-col">
            <SwitchLabel
              as="span"
              class="text-sm font-medium leading-6 text-zinc-100"
              passive
            >
              {{ $t("library.admin.import.version.updateMode") }}
            </SwitchLabel>
            <SwitchDescription as="span" class="text-sm text-zinc-400">
              {{ $t("library.admin.import.version.updateModeDesc") }}
            </SwitchDescription>
          </span>
          <Switch
            v-model="form.delta"
            :class="[
              form.delta ? 'bg-blue-600' : 'bg-zinc-900',
              'relative inline-flex h-6 w-11 flex-shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-blue-600 focus:ring-offset-2',
            ]"
          >
            <span
              aria-hidden="true"
              :class="[
                form.delta ? 'translate-x-5' : 'translate-x-0',
                'pointer-events-none inline-block h-5 w-5 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out',
              ]"
            />
          </Switch>
        </SwitchGroup>

        <div v-if="error" class="w-fit rounded-md bg-red-600/10 p-4">
          <div class="flex">
            <div class="flex-shrink-0">
              <XCircleIcon class="h-5 w-5 text-red-600" aria-hidden="true" />
            </div>
            <div class="ml-3">
              <h3 class="text-sm font-medium text-red-600">{{ error }}</h3>
            </div>
          </div>
        </div>
      </div>
    </template>
    <template #buttons>
      <LoadingButton
        type="button"
        :loading="saving"
        class="inline-flex w-full shadow-sm sm:ml-3 sm:w-auto"
        @click="() => save()"
      >
        {{ $t("common.save") }}
      </LoadingButton>
      <button
        type="button"
        class="mt-3 inline-flex w-full justify-center rounded-md bg-zinc-900 px-3 py-2 text-sm font-semibold text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-700 hover:bg-zinc-950 sm:mt-0 sm:w-auto"
        @click="open = false"
      >
        {{ $t("cancel") }}
      </button>
    </template>
  </ModalTemplate>
</template>

<script setup lang="ts">
import {
  Disclosure,
  DisclosureButton,
  DisclosurePanel,
  Switch,
  SwitchDescription,
  SwitchGroup,
  SwitchLabel,
} from "@headlessui/vue";
import { TrashIcon, MinusIcon, PlusIcon } from "@heroicons/vue/20/solid";
import { XCircleIcon } from "@heroicons/vue/16/solid";
import type { H3Error } from "h3";
import type { SerializeObject } from "nitropack";
import { GameType } from "~/prisma/client/enums";
import type { ImportVersion } from "~/server/api/v1/admin/import/version/index.post";
import type { AdminFetchGameType } from "~/server/api/v1/admin/game/[id]/index.get";

const open = defineModel<boolean>({ required: true });

const props = defineProps<{
  gameId: string;
  gameType: GameType;
  version: SerializeObject<AdminFetchGameType>["versions"][number];
}>();

const emit = defineEmits<{ saved: [] }>();

const { t } = useI18n();

type FormLaunch = (typeof ImportVersion.infer)["launches"][number];
type FormSetup = (typeof ImportVersion.infer)["setups"][number];

interface VersionForm {
  displayName: string;
  delta: boolean;
  onlySetup: boolean;
  launches: FormLaunch[];
  setups: FormSetup[];
}

function buildForm(): VersionForm {
  return {
    displayName: props.version.displayName ?? "",
    delta: props.version.delta,
    onlySetup: props.version.onlySetup,
    launches: props.version.launches.map((l) => ({
      name: l.name,
      launch: l.command,
      platform: l.platform,
      emulatorId: l.emulatorId ?? undefined,
      suggestions: l.emulatorSuggestions,
    })),
    setups: props.version.setups.map((s) => ({
      launch: s.command,
      platform: s.platform,
    })),
  };
}

const form = ref<VersionForm>(buildForm());

watch(open, (isOpen) => {
  if (isOpen) form.value = buildForm();
});

const saving = ref(false);
const error = ref<string | undefined>();

async function save() {
  saving.value = true;
  error.value = undefined;
  try {
    await $dropFetch("/api/v1/admin/game/:id/versions/config", {
      method: "PATCH",
      params: { id: props.gameId },
      body: {
        versionId: props.version.versionId,
        displayName: form.value.displayName || undefined,
        delta: form.value.delta,
        onlySetup: form.value.onlySetup,
        launches: form.value.launches,
        setups: form.value.setups,
      },
    });
    open.value = false;
    emit("saved");
  } catch (e) {
    error.value = (e as H3Error)?.statusMessage ?? t("errors.unknown");
  } finally {
    saving.value = false;
  }
}
</script>
