<template>
  <div class="w-full">
    <div class="w-full flex flex-wrap justify-between items-center gap-4">
      <div>
        <h2
          class="mt-2 text-xl font-semibold tracking-tight text-zinc-100 sm:text-3xl"
        >
          {{ $t("clients.admin.logs.title") }}
        </h2>
        <p
          class="mt-2 max-w-2xl text-pretty text-sm font-medium text-zinc-400 sm:text-md/8"
        >
          {{ $t("clients.admin.logs.description") }}
        </p>
      </div>
      <button
        type="button"
        class="inline-flex items-center gap-x-2 rounded-md bg-zinc-800 px-3 py-2 text-sm font-semibold text-zinc-100 ring-1 ring-zinc-700 hover:bg-zinc-700"
        :aria-expanded="showSettings"
        @click="showSettings = !showSettings"
      >
        <Cog6ToothIcon class="size-4" aria-hidden="true" />
        {{ $t("clients.admin.logs.settings.title") }}
      </button>
    </div>

    <!-- collection settings -->
    <div
      v-if="showSettings"
      class="mt-6 grid gap-4 rounded-xl border border-zinc-800 p-4 sm:grid-cols-3"
    >
      <div class="flex items-start justify-between gap-4 sm:col-span-3">
        <div>
          <div class="text-sm font-medium text-zinc-100">
            {{ $t("clients.admin.logs.settings.autoSend") }}
          </div>
          <div class="text-xs text-zinc-400">
            {{ $t("clients.admin.logs.settings.autoSendHint") }}
          </div>
        </div>
        <button
          type="button"
          role="switch"
          :aria-checked="settings.autoSend"
          :class="[
            settings.autoSend ? 'bg-blue-600' : 'bg-zinc-700',
            'relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full transition-colors',
          ]"
          @click="saveSettings({ autoSend: !settings.autoSend })"
        >
          <span class="sr-only">{{
            $t("clients.admin.logs.settings.autoSend")
          }}</span>
          <span
            :class="[
              settings.autoSend ? 'translate-x-5' : 'translate-x-0',
              'pointer-events-none mt-0.5 ml-0.5 inline-block size-5 rounded-full bg-white shadow transition',
            ]"
          />
        </button>
      </div>
      <div>
        <label
          for="logs-retention"
          class="block text-sm font-medium text-zinc-100"
          >{{ $t("clients.admin.logs.settings.retention") }}</label
        >
        <select
          id="logs-retention"
          :value="settings.retentionDays"
          class="mt-2 block w-full rounded-md border-0 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 ring-1 ring-inset ring-zinc-700 focus:ring-2 focus:ring-blue-500"
          @change="
            saveSettings({
              retentionDays: Number(($event.target as HTMLSelectElement).value),
            })
          "
        >
          <option v-for="days in retentionOptions" :key="days" :value="days">
            {{
              days === 0
                ? $t("clients.admin.logs.settings.forever")
                : $t("clients.admin.logs.settings.days", days)
            }}
          </option>
        </select>
      </div>
      <div>
        <label
          for="logs-per-client"
          class="block text-sm font-medium text-zinc-100"
          >{{ $t("clients.admin.logs.settings.perClient") }}</label
        >
        <input
          id="logs-per-client"
          type="number"
          min="0"
          max="1000"
          :value="settings.perClient"
          class="mt-2 block w-full rounded-md border-0 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 ring-1 ring-inset ring-zinc-700 focus:ring-2 focus:ring-blue-500"
          @change="
            saveSettings({
              perClient: Number(($event.target as HTMLInputElement).value),
            })
          "
        />
        <p class="mt-1 text-xs text-zinc-500">
          {{ $t("clients.admin.logs.settings.perClientHint") }}
        </p>
      </div>
      <div>
        <label
          for="logs-max-size"
          class="block text-sm font-medium text-zinc-100"
          >{{ $t("clients.admin.logs.settings.maxSize") }}</label
        >
        <input
          id="logs-max-size"
          type="number"
          min="1"
          max="500"
          :value="settings.maxSizeMB"
          class="mt-2 block w-full rounded-md border-0 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 ring-1 ring-inset ring-zinc-700 focus:ring-2 focus:ring-blue-500"
          @change="
            saveSettings({
              maxSizeMB: Number(($event.target as HTMLInputElement).value),
            })
          "
        />
      </div>
    </div>

    <!-- latest bundle per client -->
    <template v-if="clients.length > 0">
      <h3 class="mt-8 text-sm font-semibold text-zinc-300">
        {{ $t("clients.admin.logs.clients.title") }}
      </h3>
      <p class="mt-1 text-xs text-zinc-500">
        {{ $t("clients.admin.logs.clients.hint") }}
      </p>
      <div class="mt-3 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <button
          v-for="client in clients"
          :key="client.key"
          type="button"
          :aria-pressed="clientFilter === client.key"
          :class="[
            clientFilter === client.key
              ? 'bg-zinc-800 ring-2 ring-blue-500'
              : 'bg-zinc-800/60 ring-1 ring-zinc-700/60 hover:bg-zinc-800',
            'rounded-lg px-4 py-3 text-left transition-colors',
          ]"
          @click="
            clientFilter = clientFilter === client.key ? undefined : client.key
          "
        >
          <div class="flex items-center justify-between gap-2">
            <span class="truncate text-xs text-zinc-400">
              {{
                $t("clients.admin.logs.clients.versionArch", {
                  arch: client.latest.arch,
                  version: client.latest.version,
                })
              }}
            </span>
            <ClientLogTriggerBadge :trigger="client.latest.trigger" />
          </div>
          <div class="mt-1 truncate text-lg font-semibold text-zinc-100">
            {{ client.latest.clientName }}
          </div>
          <div class="text-xs text-zinc-500 tabular-nums">
            {{
              $t(
                "clients.admin.logs.clients.lastSent",
                {
                  date: formatReleaseDate(client.latest.receivedAt),
                  count: client.count,
                },
                client.count,
              )
            }}
          </div>
        </button>
      </div>
    </template>

    <!-- filters -->
    <div class="mt-8 flex flex-wrap items-center gap-3">
      <div class="min-w-0 grow sm:max-w-xs">
        <label for="logs-search" class="sr-only">{{
          $t("clients.admin.logs.search")
        }}</label>
        <input
          id="logs-search"
          v-model="query"
          type="search"
          :placeholder="$t('clients.admin.logs.search')"
          class="block w-full rounded-md border-0 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 placeholder:text-zinc-500 ring-1 ring-inset ring-zinc-700 focus:ring-2 focus:ring-blue-500"
        />
      </div>
      <div class="flex flex-wrap gap-2">
        <button
          v-for="option in triggerOptions"
          :key="option.value ?? 'all'"
          type="button"
          :aria-pressed="triggerFilter === option.value"
          :class="[
            triggerFilter === option.value
              ? 'bg-blue-600 text-white ring-blue-600'
              : 'bg-zinc-800 text-zinc-300 ring-zinc-700 hover:bg-zinc-700',
            'rounded-full px-3 py-1 text-xs font-medium ring-1 ring-inset',
          ]"
          @click="triggerFilter = option.value"
        >
          {{ option.label }}
        </button>
      </div>
    </div>

    <div
      class="mt-4 overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900 shadow-sm"
    >
      <div class="overflow-x-auto">
        <table class="min-w-full divide-y divide-zinc-800">
          <thead>
            <tr class="bg-zinc-800/50 text-left text-sm text-zinc-100">
              <th scope="col" class="py-3.5 pl-4 pr-3 font-semibold sm:pl-6">
                <span class="inline-flex items-center gap-1">
                  {{ $t("clients.admin.logs.table.received") }}
                  <ArrowDownIcon
                    class="size-3.5 text-blue-400"
                    :aria-label="$t('settings.admin.updater.table.newestFirst')"
                  />
                </span>
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                {{ $t("clients.admin.logs.table.client") }}
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                {{ $t("clients.admin.logs.table.user") }}
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                {{ $t("clients.admin.logs.table.reason") }}
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                {{ $t("clients.admin.logs.table.version") }}
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                {{ $t("clients.admin.logs.table.session") }}
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                {{ $t("clients.admin.logs.table.problems") }}
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                {{ $t("clients.admin.logs.table.size") }}
              </th>
              <th scope="col" class="relative py-3.5 pl-3 pr-4 sm:pr-6">
                <span class="sr-only">{{
                  $t("clients.admin.logs.table.open")
                }}</span>
              </th>
            </tr>
          </thead>
          <tbody class="divide-y divide-zinc-800 text-sm tabular-nums">
            <tr
              v-for="bundle in filtered"
              :key="bundle.id"
              class="cursor-pointer transition-colors duration-150 hover:bg-zinc-800/50"
              @click="navigateTo(`/admin/clients/logs/${bundle.id}`)"
            >
              <td
                class="whitespace-nowrap py-4 pl-4 pr-3 text-zinc-400 sm:pl-6"
              >
                {{ formatReleaseDate(bundle.receivedAt) }}
              </td>
              <td class="whitespace-nowrap px-3 py-4 font-medium text-zinc-100">
                {{ bundle.clientName }}
              </td>
              <td class="whitespace-nowrap px-3 py-4 text-zinc-400">
                {{ bundle.userName }}
              </td>
              <td class="whitespace-nowrap px-3 py-4">
                <ClientLogTriggerBadge :trigger="bundle.trigger" />
                <span v-if="bundle.gameName" class="ml-2 text-xs text-zinc-400">
                  {{ bundle.gameName }}
                </span>
              </td>
              <td class="whitespace-nowrap px-3 py-4">
                <span class="text-zinc-300">{{ bundle.version }}</span>
                <span
                  class="ml-1.5 inline-flex items-center rounded-md bg-blue-400/10 px-1.5 py-0.5 text-[11px] font-medium text-blue-400 ring-1 ring-inset ring-blue-400/20"
                  >{{ bundle.arch }}</span
                >
              </td>
              <td class="whitespace-nowrap px-3 py-4 text-zinc-400">
                {{ bundle.session }}
              </td>
              <td class="whitespace-nowrap px-3 py-4">
                <span v-if="bundle.errors" class="text-red-400">{{
                  $t("clients.admin.logs.errors", bundle.errors)
                }}</span>
                <span v-if="bundle.warnings" class="ml-2 text-amber-300">{{
                  $t("clients.admin.logs.warnings", bundle.warnings)
                }}</span>
                <span
                  v-if="!bundle.errors && !bundle.warnings"
                  class="text-zinc-600"
                  >{{ $t("chars.emDash") }}</span
                >
              </td>
              <td class="whitespace-nowrap px-3 py-4 text-zinc-400">
                {{ formatReleaseSize(bundle.size) }}
                <span class="text-zinc-600">{{
                  $t("clients.admin.logs.fileCount", bundle.files.length)
                }}</span>
              </td>
              <td class="py-4 pl-3 pr-4 text-right sm:pr-6">
                <NuxtLink
                  :to="`/admin/clients/logs/${bundle.id}`"
                  class="text-zinc-500 hover:text-zinc-200"
                  @click.stop
                >
                  <ChevronRightIcon class="size-5" aria-hidden="true" />
                  <span class="sr-only">{{
                    $t("clients.admin.logs.table.open")
                  }}</span>
                </NuxtLink>
              </td>
            </tr>
            <tr v-if="filtered.length === 0">
              <td colspan="9" class="py-8 text-center text-sm text-zinc-400">
                {{
                  data.bundles.length === 0
                    ? $t("clients.admin.logs.table.empty")
                    : $t("clients.admin.logs.table.noMatches")
                }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { Cog6ToothIcon } from "@heroicons/vue/24/outline";
import { ArrowDownIcon, ChevronRightIcon } from "@heroicons/vue/20/solid";
import type { ClientLogTriggerSlug } from "~/utils/clientLogs";

definePageMeta({
  layout: "admin",
});

const { t } = useI18n();

useHead({ title: t("clients.admin.logs.pageTitle") });

const data = await $dropFetch("/api/v1/admin/clientlogs");
const settings = ref(data.settings);
const showSettings = ref(false);

const retentionOptions = computed(() =>
  [...new Set([7, 30, 90, 365, 0, settings.value.retentionDays])].sort(
    (a, b) => (a || Infinity) - (b || Infinity),
  ),
);

async function saveSettings(patch: Partial<typeof data.settings>) {
  settings.value = await $dropFetch("/api/v1/admin/clientlogs/settings", {
    method: "PATCH",
    body: patch,
    failTitle: t("clients.admin.logs.settings.failed"),
  });
}

const clients = computed(() => {
  const byClient = new Map<
    string,
    { key: string; latest: (typeof data.bundles)[number]; count: number }
  >();
  for (const bundle of data.bundles) {
    const key = bundle.clientId ?? bundle.clientName;
    const existing = byClient.get(key);
    if (existing) existing.count++;
    else byClient.set(key, { key, latest: bundle, count: 1 });
  }
  return [...byClient.values()];
});

const clientFilter = ref<string>();
const triggerFilter = ref<ClientLogTriggerSlug>();
const query = ref("");

const triggerLabels = useClientLogTriggerLabels();
const triggerOptions = computed(() => [
  { value: undefined, label: t("clients.admin.logs.triggers.all") },
  ...(["launch", "crash", "manual"] as const).map((value) => ({
    value,
    label: triggerLabels.value[value],
  })),
]);

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  return data.bundles.filter(
    (bundle) =>
      (!clientFilter.value ||
        (bundle.clientId ?? bundle.clientName) === clientFilter.value) &&
      (!triggerFilter.value || bundle.trigger === triggerFilter.value) &&
      (!q ||
        [bundle.clientName, bundle.userName, bundle.gameName, bundle.version]
          .join(" ")
          .toLowerCase()
          .includes(q)),
  );
});
</script>
