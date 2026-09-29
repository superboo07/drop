<template>
  <div class="flex flex-col gap-4">
    <p
      v-if="candidates.length == 0"
      class="text-sm text-zinc-500 uppercase font-display font-bold"
    >
      {{ $t("library.admin.import.version.updateBase.noCandidates") }}
    </p>
    <Listbox v-else v-model="baseVersionId" as="div">
      <ListboxLabel class="block text-sm font-medium leading-6 text-zinc-100">
        {{ $t("library.admin.import.version.updateBase.label") }}
      </ListboxLabel>
      <p class="text-xs text-zinc-400">
        {{ $t("library.admin.import.version.updateBase.desc") }}
      </p>
      <div class="relative mt-2">
        <ListboxButton
          class="relative w-full cursor-default rounded-md bg-zinc-950 py-1.5 pl-3 pr-10 text-left text-zinc-100 shadow-sm ring-1 ring-inset ring-zinc-800 focus:outline-none focus:ring-2 focus:ring-blue-600 sm:text-sm sm:leading-6"
        >
          <span v-if="selectedBase" class="block truncate">
            {{ versionName(selectedBase) }}
            <span class="text-zinc-500"
              >· {{ optionDetail(selectedBase) }}</span
            >
          </span>
          <span v-else class="block truncate text-zinc-600">{{
            $t("library.admin.import.version.updateBase.select")
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
            v-for="candidate in candidates"
            :key="candidate.versionId"
            v-slot="{ active, selected }"
            as="template"
            :value="candidate.versionId"
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
              >
                {{ versionName(candidate) }}
                <span :class="active ? 'text-blue-100' : 'text-zinc-500'"
                  >· {{ optionDetail(candidate) }}</span
                >
                <span
                  v-if="candidate.versionId === newestVersionId"
                  :class="active ? 'text-blue-100' : 'text-zinc-500'"
                  >({{
                    $t("library.admin.import.version.updateBase.newest")
                  }})</span
                >
              </span>
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

    <div
      v-if="dependentNames.length > 0"
      class="text-xs rounded-lg ring-1 px-3 py-2 bg-amber-500/10 text-amber-200 ring-amber-500/30"
    >
      {{
        $t("library.admin.import.version.updateBase.dependents", [
          dependentNames.join(", "),
        ])
      }}
    </div>

    <div v-if="baseVersionId" class="flex flex-col gap-3">
      <div class="flex flex-wrap items-baseline justify-between gap-2">
        <h3 class="text-sm font-medium text-zinc-100">
          {{ $t("library.admin.import.version.updateBase.path") }}
        </h3>
        <span v-if="preview" class="text-xs text-zinc-400">{{
          $t("library.admin.import.version.updateBase.pathSummary", [
            preview.steps.length + 1,
            preview.steps[0].name,
          ])
        }}</span>
      </div>

      <p v-if="previewError" class="text-xs text-red-400">
        {{
          $t("library.admin.import.version.updateBase.previewError", [
            previewError,
          ])
        }}
      </p>
      <p v-else-if="!preview" class="text-xs text-zinc-500">
        {{ $t("library.admin.import.version.updateBase.loading") }}
      </p>
      <template v-else>
        <ol class="flex flex-col gap-2">
          <li
            v-for="step in preview.steps"
            :key="step.versionId"
            class="relative flex gap-3 items-start"
          >
            <span
              class="absolute left-[15px] top-8 -bottom-2 w-0.5 bg-zinc-700"
              aria-hidden="true"
            />
            <span
              :class="[
                step.delta ? 'border-blue-500/60' : 'border-emerald-500/70',
                'relative z-10 flex size-8 flex-shrink-0 items-center justify-center rounded-full bg-zinc-900 border-2',
              ]"
            >
              <span
                :class="[
                  step.delta ? 'bg-blue-400' : 'bg-emerald-400',
                  'size-2 rounded-full',
                ]"
              />
            </span>
            <div
              class="flex-1 min-w-0 rounded-lg bg-zinc-900 px-3 py-2 flex flex-wrap items-center gap-x-3 gap-y-1"
            >
              <span class="text-sm text-zinc-100 truncate">{{
                step.name
              }}</span>
              <span
                :class="[
                  step.delta
                    ? 'bg-blue-600/15 text-blue-300'
                    : 'bg-emerald-600/15 text-emerald-300',
                  'text-[10px] uppercase tracking-wider px-1.5 py-0.5 rounded',
                ]"
                >{{
                  step.delta
                    ? $t("library.admin.import.version.updateBase.update")
                    : $t("library.admin.import.version.updateBase.full")
                }}</span
              >
              <span class="text-xs text-zinc-400 ml-auto tabular-nums">
                {{
                  $t(
                    "library.admin.import.version.updateBase.files",
                    step.fileCount,
                  )
                }}<template v-if="step.removedCount > 0">
                  ·
                  {{
                    $t("library.admin.import.version.updateBase.removed", [
                      step.removedCount,
                    ])
                  }}</template
                >
              </span>
            </div>
          </li>
          <li class="relative flex gap-3 items-start">
            <span
              class="relative z-10 flex size-8 flex-shrink-0 items-center justify-center rounded-full bg-blue-600 text-white"
            >
              <ArrowDownTrayIcon class="size-4" aria-hidden="true" />
            </span>
            <div
              class="flex-1 min-w-0 rounded-lg bg-blue-600/10 ring-1 ring-blue-600/40 px-3 py-2 flex flex-wrap items-center gap-x-3 gap-y-1"
            >
              <span class="text-sm text-zinc-100 truncate">{{
                targetName ??
                $t("library.admin.import.version.updateBase.thisVersion")
              }}</span>
              <span
                class="text-[10px] uppercase tracking-wider bg-blue-600/25 text-blue-200 px-1.5 py-0.5 rounded"
                >{{
                  $t("library.admin.import.version.updateBase.thisVersion")
                }}</span
              >
              <span
                v-if="preview.target"
                class="text-xs text-zinc-400 ml-auto tabular-nums"
              >
                {{
                  $t("library.admin.import.version.updateBase.overwritten", [
                    preview.target.overwritten,
                  ])
                }}
                ·
                {{
                  $t("library.admin.import.version.updateBase.added", [
                    preview.target.added,
                  ])
                }}
              </span>
            </div>
          </li>
        </ol>

        <div
          v-if="skippedNames.length > 0"
          class="text-xs rounded-lg ring-1 px-3 py-2 bg-amber-500/10 text-amber-200 ring-amber-500/30"
        >
          {{
            $t("library.admin.import.version.updateBase.skipped", [
              skippedNames.join(", "),
            ])
          }}
        </div>

        <div class="bg-zinc-900 rounded-lg p-3 flex flex-col gap-1">
          <span
            class="text-xs uppercase tracking-wider text-zinc-500 font-semibold"
            >{{
              $t("library.admin.import.version.updateBase.baseInstall", [
                selectedBase ? versionName(selectedBase) : "",
              ])
            }}</span
          >
          <span class="text-sm text-zinc-100 tabular-nums">
            <template v-if="preview.baseInstallSize !== null"
              >{{ formatBytes(preview.baseInstallSize) }} ·
            </template>
            {{
              $t(
                "library.admin.import.version.updateBase.files",
                preview.baseFileCount,
              )
            }}
          </span>
          <span class="text-xs text-zinc-400">{{
            $t("library.admin.import.version.updateBase.baseInstallDesc")
          }}</span>
        </div>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  Listbox,
  ListboxButton,
  ListboxLabel,
  ListboxOption,
  ListboxOptions,
} from "@headlessui/vue";
import {
  ArrowDownTrayIcon,
  CheckIcon,
  ChevronUpDownIcon,
} from "@heroicons/vue/20/solid";
import type { H3Error } from "h3";
import { formatBytes } from "~/server/internal/utils/files";

export type UpdateBaseCandidate = {
  versionId: string;
  displayName: string | null;
  versionPath: string | null;
  versionIndex: number;
  delta: boolean;
  baseVersionId: string | null;
};

export type UpdateBaseTarget =
  | { versionId: string }
  | { type: "depot" | "local"; identifier: string };

const props = defineProps<{
  gameId: string;
  versions: UpdateBaseCandidate[];
  // The version being edited: it and everything built on it can't be a base.
  editingVersionId?: string;
  // Whose files to compare against the base, for the overwritten/new counts.
  target?: UpdateBaseTarget;
  targetName?: string;
}>();

const baseVersionId = defineModel<string | null | undefined>({
  required: true,
});

const { t } = useI18n();

function versionName(v: UpdateBaseCandidate) {
  return v.displayName ?? v.versionPath ?? v.versionId;
}

const byId = computed(
  () => new Map(props.versions.map((v) => [v.versionId, v])),
);

function optionDetail(v: UpdateBaseCandidate) {
  if (!v.delta) return t("library.admin.import.version.updateBase.fullOption");
  const base = v.baseVersionId ? byId.value.get(v.baseVersionId) : undefined;
  return t("library.admin.import.version.updateBase.updateOf", [
    base ? versionName(base) : "?",
  ]);
}

// Everything whose base chain runs through the version being edited.
const dependents = computed(() => {
  if (!props.editingVersionId) return [];
  const reached = new Set([props.editingVersionId]);
  const found: UpdateBaseCandidate[] = [];
  let grew = true;
  while (grew) {
    grew = false;
    for (const v of props.versions) {
      if (reached.has(v.versionId) || !v.delta || !v.baseVersionId) continue;
      if (!reached.has(v.baseVersionId)) continue;
      reached.add(v.versionId);
      found.push(v);
      grew = true;
    }
  }
  return found;
});
const dependentNames = computed(() => dependents.value.map(versionName));

const candidates = computed(() => {
  const blocked = new Set([
    props.editingVersionId,
    ...dependents.value.map((v) => v.versionId),
  ]);
  return props.versions
    .filter((v) => !blocked.has(v.versionId))
    .sort((a, b) => b.versionIndex - a.versionIndex);
});

// Only labelled when it's actually the game's newest version, not just the
// first one left after excluding the version being edited and its dependents.
const newestVersionId = computed(
  () =>
    props.versions
      .filter((v) => v.versionId !== props.editingVersionId)
      .sort((a, b) => b.versionIndex - a.versionIndex)[0]?.versionId,
);

const selectedBase = computed(() =>
  baseVersionId.value ? byId.value.get(baseVersionId.value) : undefined,
);

// Default to the newest version, same as the old implicit behaviour.
watch(
  candidates,
  (list) => {
    if (list.length == 0) return;
    if (!list.some((c) => c.versionId === baseVersionId.value))
      baseVersionId.value = list[0].versionId;
  },
  { immediate: true },
);

type PathPreview = {
  steps: Array<{
    versionId: string;
    name: string;
    delta: boolean;
    fileCount: number;
    removedCount: number;
  }>;
  baseInstallSize: number | null;
  baseFileCount: number;
  target: { fileCount: number; overwritten: number; added: number } | null;
};

const preview = ref<PathPreview | undefined>();
const previewError = ref<string | undefined>();
let previewRequest = 0;

watch(
  () => [baseVersionId.value, JSON.stringify(props.target ?? null)] as const,
  async ([base]) => {
    const request = ++previewRequest;
    preview.value = undefined;
    previewError.value = undefined;
    if (!base) return;
    try {
      const result = await $dropFetch("/api/v1/admin/game/:id/versions/path", {
        params: { id: props.gameId },
        query: { base, ...(props.target ?? {}) },
      });
      if (request === previewRequest) preview.value = result as PathPreview;
    } catch (e) {
      if (request === previewRequest)
        previewError.value =
          (e as H3Error)?.statusMessage ?? t("errors.unknown");
    }
  },
  { immediate: true },
);

// Versions newer than the chosen base that this path doesn't go through.
const skippedNames = computed(() => {
  const base = selectedBase.value;
  if (!base || !preview.value) return [];
  const inPath = new Set(preview.value.steps.map((s) => s.versionId));
  const ignored = new Set([
    props.editingVersionId,
    ...dependents.value.map((v) => v.versionId),
  ]);
  return props.versions
    .filter(
      (v) =>
        v.versionIndex > base.versionIndex &&
        !inPath.has(v.versionId) &&
        !ignored.has(v.versionId),
    )
    .sort((a, b) => b.versionIndex - a.versionIndex)
    .map(versionName);
});
</script>
