<template>
  <div class="w-full">
    <NuxtLink
      to="/admin/clients/logs"
      class="inline-flex items-center gap-2 text-sm text-zinc-400 hover:text-zinc-200"
    >
      <ArrowLeftIcon class="size-4" aria-hidden="true" />
      {{ $t("clients.admin.logs.title") }}
    </NuxtLink>

    <div class="mt-2 flex flex-wrap items-center justify-between gap-4">
      <div class="flex min-w-0 flex-wrap items-center gap-3">
        <h2
          class="min-w-0 break-words text-xl font-semibold tracking-tight text-zinc-100 sm:text-3xl"
        >
          {{
            bundle.gameName
              ? $t("clients.admin.logs.detail.titleWithGame", {
                  client: bundle.clientName,
                  game: bundle.gameName,
                })
              : bundle.clientName
          }}
        </h2>
        <ClientLogTriggerBadge :trigger="bundle.trigger" />
        <span
          class="inline-flex items-center rounded-md bg-blue-400/10 px-2 py-1 text-xs font-medium text-blue-400 ring-1 ring-inset ring-blue-400/20"
        >
          {{ bundle.arch }}
        </span>
      </div>
      <a
        :href="`/api/v1/admin/clientlogs/${bundle.id}/download`"
        class="inline-flex items-center gap-x-2 rounded-md bg-zinc-800 px-3 py-2 text-sm font-semibold text-zinc-100 ring-1 ring-zinc-700 hover:bg-zinc-700"
      >
        <ArrowDownTrayIcon class="size-4" aria-hidden="true" />
        {{ $t("clients.admin.logs.detail.download") }}
      </a>
    </div>

    <div class="mt-8 grid gap-8 xl:grid-cols-[minmax(0,20rem)_minmax(0,1fr)]">
      <div class="flex min-w-0 flex-col gap-6">
        <div
          v-if="bundle.summary"
          class="rounded-xl border border-red-500/30 bg-red-500/5 p-4"
        >
          <div class="text-sm font-semibold text-red-300">
            {{ bundle.summary.title }}
          </div>
          <div v-if="bundle.summary.detail" class="mt-1 text-xs text-zinc-300">
            {{ bundle.summary.detail }}
          </div>
          <div
            v-if="bundle.summary.lastError"
            class="mt-2 break-words rounded-md bg-zinc-950/60 px-2 py-1.5 font-mono text-[11px] text-red-300"
          >
            {{ bundle.summary.lastError }}
          </div>
        </div>

        <dl
          class="divide-y divide-zinc-800 rounded-xl border border-zinc-800 text-sm"
        >
          <div
            v-for="row in facts"
            :key="row.label"
            class="flex justify-between gap-4 px-4 py-3"
          >
            <dt class="shrink-0 text-zinc-400">{{ row.label }}</dt>
            <dd
              class="min-w-0 break-words text-right text-zinc-100 tabular-nums"
            >
              {{ row.value }}
            </dd>
          </div>
        </dl>

        <div v-if="bundle.environment.length > 0">
          <h3 class="text-sm font-semibold text-zinc-300">
            {{ $t("clients.admin.logs.detail.environment") }}
          </h3>
          <p class="mt-1 text-xs text-zinc-500">
            {{ $t("clients.admin.logs.detail.environmentHint") }}
          </p>
          <dl
            class="mt-3 divide-y divide-zinc-800 rounded-xl border border-zinc-800 font-mono text-xs"
          >
            <div
              v-for="[name, value, flagged] in bundle.environment"
              :key="name"
              class="flex justify-between gap-4 px-4 py-2"
            >
              <dt class="shrink-0 text-zinc-400">{{ name }}</dt>
              <dd
                :class="[
                  flagged ? 'text-amber-300' : 'text-zinc-200',
                  'min-w-0 break-all text-right',
                ]"
              >
                {{ value }}
              </dd>
            </div>
          </dl>
        </div>

        <div class="rounded-xl border border-red-500/30 p-4">
          <div class="flex items-center justify-between gap-4">
            <div>
              <div class="text-sm font-medium text-zinc-100">
                {{ $t("common.delete") }}
              </div>
              <div class="text-xs text-zinc-400">
                {{ $t("clients.admin.logs.detail.delete.hint") }}
              </div>
            </div>
            <button
              type="button"
              class="shrink-0 rounded-md bg-red-600 px-2.5 py-1.5 text-xs font-semibold text-white hover:bg-red-500"
              @click="confirmDelete"
            >
              {{ $t("common.delete") }}
            </button>
          </div>
        </div>
      </div>

      <!-- log viewer -->
      <div class="min-w-0">
        <div
          class="flex flex-wrap gap-2"
          role="tablist"
          :aria-label="$t('clients.admin.logs.detail.files')"
        >
          <button
            v-for="(tab, index) in bundle.files"
            :key="index"
            type="button"
            role="tab"
            :aria-selected="index === currentFile"
            :class="[
              index === currentFile
                ? 'bg-zinc-800 text-zinc-100 ring-blue-500'
                : 'bg-zinc-900 text-zinc-400 ring-zinc-800 hover:text-zinc-200',
              'inline-flex items-center gap-2 rounded-md px-3 py-1.5 text-xs font-medium ring-1 ring-inset',
            ]"
            @click="currentFile = index"
          >
            <span class="font-mono">{{ tab.name }}</span>
            <span v-if="tab.label" class="text-zinc-500">{{ tab.label }}</span>
            <span
              v-if="tab.errors"
              class="rounded bg-red-400/15 px-1.5 text-[10px] text-red-400"
              >{{ tab.errors }}</span
            >
          </button>
        </div>

        <div
          v-if="file"
          class="mt-3 overflow-hidden rounded-xl border border-zinc-800 bg-zinc-950"
        >
          <div
            class="flex flex-wrap items-center gap-2 border-b border-zinc-800 bg-zinc-900 px-3 py-2"
          >
            <span
              class="min-w-0 truncate font-mono text-xs text-zinc-500"
              :title="file.path"
              >{{ file.path }}</span
            >
            <div class="ml-auto flex flex-wrap items-center gap-2">
              <div class="flex flex-wrap gap-1.5">
                <button
                  v-for="level in shownLevels"
                  :key="level"
                  type="button"
                  :aria-pressed="levelsOn[level]"
                  :class="[
                    levelsOn[level]
                      ? `bg-zinc-800 ring-zinc-600 ${levelStyles[level]}`
                      : 'text-zinc-600 line-through ring-zinc-800',
                    'rounded px-2 py-0.5 text-[11px] font-semibold ring-1 ring-inset',
                  ]"
                  @click="levelsOn[level] = !levelsOn[level]"
                >
                  {{ level }}
                  <span class="font-normal text-zinc-500">{{
                    levelCounts[level] ?? 0
                  }}</span>
                </button>
              </div>
              <label for="log-search" class="sr-only">{{
                $t("clients.admin.logs.detail.search")
              }}</label>
              <input
                id="log-search"
                v-model="logQuery"
                type="search"
                :placeholder="$t('clients.admin.logs.detail.search')"
                class="w-40 rounded-md border-0 bg-zinc-800 px-2.5 py-1 text-xs text-zinc-100 placeholder:text-zinc-500 ring-1 ring-inset ring-zinc-700 focus:ring-2 focus:ring-blue-500"
              />
              <button
                type="button"
                :disabled="errorLines.length === 0"
                class="rounded-md bg-red-400/10 px-2.5 py-1 text-xs font-medium text-red-400 ring-1 ring-inset ring-red-400/20 hover:bg-red-400/20 disabled:opacity-40"
                @click="nextError"
              >
                {{ $t("clients.admin.logs.detail.nextError") }}
              </button>
            </div>
          </div>
          <div ref="scroller" class="max-h-[40rem] overflow-auto">
            <div
              v-if="loading"
              class="px-4 py-6 text-center text-sm text-zinc-500"
            >
              {{ $t("common.srLoading") }}
            </div>
            <div
              v-else-if="loadError"
              class="px-4 py-6 text-center text-sm text-red-400"
            >
              {{ loadError }}
            </div>
            <table v-else class="w-full font-mono text-[12px] leading-5">
              <tbody>
                <tr
                  v-for="line in visibleLines"
                  :id="`line-${line.number}`"
                  :key="line.number"
                  :class="[
                    line.number === highlighted
                      ? 'bg-blue-400/15'
                      : line.level === 'ERROR'
                        ? 'bg-red-400/[0.07]'
                        : line.level === 'WARN'
                          ? 'bg-amber-400/[0.05]'
                          : '',
                  ]"
                >
                  <td
                    class="select-none border-r border-zinc-800 px-3 text-right align-top text-zinc-600"
                  >
                    {{ line.number }}
                  </td>
                  <td class="whitespace-pre-wrap break-all px-3">
                    <template v-if="line.drop">
                      <span class="text-zinc-500" :title="line.drop.time">{{
                        line.drop.time.slice(11, 23)
                      }}</span>
                      <span
                        :class="[
                          levelStyles[line.drop.level],
                          'ml-2 font-semibold',
                        ]"
                        >{{ line.drop.level.padEnd(5) }}</span
                      >
                      <span class="ml-2 text-zinc-500">{{
                        line.drop.source
                      }}</span>
                      <span class="ml-2 text-zinc-200">{{
                        line.drop.message
                      }}</span>
                    </template>
                    <span
                      v-else
                      :class="
                        line.level ? levelStyles[line.level] : 'text-zinc-300'
                      "
                      >{{ line.text }}</span
                    >
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
        <p v-if="file" class="mt-2 text-xs text-zinc-500 tabular-nums">
          {{ $t("clients.admin.logs.detail.lineCount", lines.length) }}
          <template v-if="hiddenCount > 0">
            {{ $t("chars.middot") }}
            {{
              $t("clients.admin.logs.detail.hiddenByFilters", {
                count: hiddenCount,
              })
            }}
          </template>
          <template v-if="file.truncated">
            {{ $t("chars.middot") }}
            {{ $t("clients.admin.logs.detail.truncated") }}
          </template>
          <template v-if="clipped > 0">
            {{ $t("chars.middot") }}
            {{
              $t("clients.admin.logs.detail.clipped", {
                count: maxRenderedLines,
              })
            }}
            <button
              type="button"
              class="text-blue-400 hover:text-blue-300"
              @click="showAll = true"
            >
              {{ $t("clients.admin.logs.detail.showAll") }}
            </button>
          </template>
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ArrowDownTrayIcon } from "@heroicons/vue/24/outline";
import { ArrowLeftIcon } from "@heroicons/vue/20/solid";
import {
  classifyLogLine,
  clientLogLevels,
  parseDropLogLine,
  type ClientLogLevel,
} from "~/utils/clientLogs";

definePageMeta({
  layout: "admin",
});

const { t } = useI18n();
const route = useRoute();
const id = route.params.id as string;

const bundle = await $dropFetch("/api/v1/admin/clientlogs/:id", {
  params: { id },
});

useHead({
  title: t("clients.admin.logs.detail.pageTitle", [bundle.clientName]),
});

const facts = computed(() =>
  [
    [t("clients.admin.logs.table.client"), bundle.clientName],
    [t("clients.admin.logs.table.user"), bundle.userName],
    [
      t("clients.admin.logs.table.received"),
      formatReleaseDate(bundle.receivedAt),
    ],
    [t("clients.admin.logs.table.version"), bundle.version],
    [t("clients.admin.logs.table.session"), bundle.session],
    [t("clients.admin.logs.detail.game"), bundle.gameName ?? bundle.gameId],
    [
      t("clients.admin.logs.detail.bundle"),
      `${formatReleaseSize(bundle.size)} ${t("clients.admin.logs.fileCount", bundle.files.length)}`,
    ],
  ]
    .filter(([, value]) => value)
    .map(([label, value]) => ({ label: label!, value: value! })),
);

const levelStyles: Record<ClientLogLevel, string> = {
  ERROR: "text-red-400",
  WARN: "text-amber-300",
  INFO: "text-sky-300",
  DEBUG: "text-zinc-500",
  TRACE: "text-zinc-600",
};

// Start on the file with the most errors - for a failed launch that's the
// game's own output, which is where the reason usually is
const currentFile = ref(
  bundle.files.reduce(
    (best, file, index) =>
      file.errors > (bundle.files[best]?.errors ?? 0) ? index : best,
    0,
  ),
);
const file = computed(() => bundle.files[currentFile.value]);

const content = ref("");
const loading = ref(false);
const loadError = ref<string>();

async function loadFile(index: number) {
  loading.value = true;
  loadError.value = undefined;
  try {
    content.value = await $fetch<string>(
      `/api/v1/admin/clientlogs/${id}/files/${index}`,
      { responseType: "text" },
    );
  } catch (e) {
    content.value = "";
    loadError.value = t("clients.admin.logs.detail.loadFailed", [
      e instanceof Error ? e.message : `${e}`,
    ]);
  } finally {
    loading.value = false;
  }
}

watch(currentFile, (index) => {
  highlighted.value = undefined;
  showAll.value = false;
  loadFile(index);
});
onMounted(() => loadFile(currentFile.value));

const lines = computed(() => {
  const raw = content.value.split("\n");
  if (raw.at(-1) === "") raw.pop();
  return raw.map((text, index) => ({
    number: index + 1,
    text,
    level: classifyLogLine(text),
    drop: parseDropLogLine(text),
  }));
});

const levelCounts = computed(() => {
  const counts: Partial<Record<ClientLogLevel, number>> = {};
  for (const line of lines.value)
    if (line.level) counts[line.level] = (counts[line.level] ?? 0) + 1;
  return counts;
});

// TRACE only shows up when a client ran with RUST_LOG=trace
const shownLevels = computed(() =>
  clientLogLevels.filter(
    (level) => level !== "TRACE" || levelCounts.value.TRACE,
  ),
);

const levelsOn = ref<Record<ClientLogLevel, boolean>>({
  ERROR: true,
  WARN: true,
  INFO: true,
  DEBUG: true,
  TRACE: true,
});
const logQuery = ref("");

const matchingLines = computed(() => {
  const q = logQuery.value.trim().toLowerCase();
  return lines.value.filter(
    (line) =>
      (!line.level || levelsOn.value[line.level]) &&
      (!q || line.text.toLowerCase().includes(q)),
  );
});
const hiddenCount = computed(
  () => lines.value.length - matchingLines.value.length,
);

// Rendering every line of a large log as a table row is slow; the end of a
// log is nearly always the interesting part
const maxRenderedLines = 5000;
const showAll = ref(false);
const clipped = computed(() =>
  showAll.value
    ? 0
    : Math.max(0, matchingLines.value.length - maxRenderedLines),
);
const visibleLines = computed(() => matchingLines.value.slice(clipped.value));

const errorLines = computed(() =>
  visibleLines.value.filter((line) => line.level === "ERROR"),
);
const highlighted = ref<number>();
const scroller = ref<HTMLElement>();

function nextError() {
  const errors = errorLines.value;
  if (errors.length === 0) return;
  const next =
    errors.find((line) => line.number > (highlighted.value ?? 0)) ?? errors[0];
  highlighted.value = next.number;
  scroller.value
    ?.querySelector(`#line-${next.number}`)
    ?.scrollIntoView({ block: "center" });
}

function confirmDelete() {
  createModal(
    ModalType.Confirmation,
    {
      title: t("clients.admin.logs.detail.delete.confirmTitle"),
      description: t("clients.admin.logs.detail.delete.confirmDescription"),
      buttonText: t("common.delete"),
    },
    async (event, close) => {
      if (event !== "confirm") return close();
      try {
        await $dropFetch("/api/v1/admin/clientlogs/:id", {
          method: "DELETE",
          params: { id },
          failTitle: t("clients.admin.logs.detail.delete.failed"),
        });
      } catch {
        return close();
      }
      close();
      await navigateTo("/admin/clients/logs");
    },
  );
}
</script>
