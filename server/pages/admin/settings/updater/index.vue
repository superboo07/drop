<template>
  <div class="w-full">
    <div class="w-full flex flex-wrap justify-between items-center gap-4">
      <div>
        <h2
          class="mt-2 text-xl font-semibold tracking-tight text-zinc-100 sm:text-3xl"
        >
          {{ $t("settings.admin.updater.title") }}
        </h2>
        <p
          class="mt-2 text-pretty text-sm font-medium text-zinc-400 sm:text-md/8"
        >
          {{ $t("settings.admin.updater.description") }}
        </p>
      </div>
      <NuxtLink
        to="/admin/settings/updater/upload"
        class="inline-flex items-center gap-x-2 rounded-md bg-blue-600 px-3 py-2 text-sm font-semibold text-white shadow-sm hover:bg-blue-500 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600"
      >
        <CloudArrowUpIcon class="size-5" aria-hidden="true" />
        {{ $t("settings.admin.updater.upload.title") }}
      </NuxtLink>
    </div>

    <h3 class="mt-8 text-sm font-semibold text-zinc-300">
      {{ $t("settings.admin.updater.offered.title") }}
    </h3>
    <p class="mt-1 text-xs text-zinc-500">
      {{ $t("settings.admin.updater.offered.hint") }}
    </p>
    <div class="mt-3 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
      <component
        :is="target.offered ? NuxtLink : 'div'"
        v-for="target in data.targets"
        :key="`${target.target}/${target.arch}`"
        :to="
          target.offered
            ? `/admin/settings/updater/${target.offered.id}`
            : undefined
        "
        :class="[
          target.offered
            ? 'bg-zinc-800/60 ring-1 ring-zinc-700/60 hover:bg-zinc-800'
            : 'border border-dashed border-zinc-700',
          'rounded-lg px-4 py-3 transition-colors',
        ]"
      >
        <div class="flex items-center justify-between gap-2">
          <span class="text-xs text-zinc-400">
            {{
              $t("settings.admin.updater.platforms.targetArch", {
                target: targetLabels[target.target],
                arch: target.arch,
              })
            }}
          </span>
          <UpdaterBranchBadge :branch="target.branch" />
        </div>
        <template v-if="target.offered">
          <div class="mt-1 text-lg font-semibold text-zinc-100 tabular-nums">
            {{ target.offered.tag }}
          </div>
          <div class="text-xs text-zinc-500 tabular-nums">
            {{
              $t("settings.admin.updater.offered.published", [
                formatReleaseDate(target.offered.publishedAt!),
              ])
            }}
          </div>
        </template>
        <div v-else class="mt-1 text-sm font-medium text-zinc-500">
          {{ $t("settings.admin.updater.offered.nothingPublished") }}
        </div>
      </component>
      <div
        v-for="planned in plannedTargets"
        :key="planned"
        class="rounded-lg border border-dashed border-zinc-800 px-4 py-3"
      >
        <div class="text-xs text-zinc-500">{{ planned }}</div>
        <div class="mt-1 text-sm font-medium text-zinc-600">
          {{ $t("settings.admin.updater.offered.notSupported") }}
        </div>
      </div>
    </div>

    <div
      class="mt-8 overflow-hidden rounded-xl border border-zinc-800 bg-zinc-900 shadow-sm"
    >
      <div class="overflow-x-auto">
        <table class="min-w-full divide-y divide-zinc-800">
          <thead>
            <tr class="bg-zinc-800/50 text-left text-sm text-zinc-100">
              <th scope="col" class="py-3.5 pl-4 pr-3 font-semibold sm:pl-6">
                {{ $t("settings.admin.updater.table.tag") }}
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                {{ $t("settings.admin.updater.table.releaseId") }}
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                {{ $t("settings.admin.updater.table.platform") }}
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                {{ $t("settings.admin.updater.table.branch") }}
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                {{ $t("settings.admin.updater.table.size") }}
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                <span class="inline-flex items-center gap-1">
                  {{ $t("settings.admin.updater.table.uploaded") }}
                  <ArrowDownIcon
                    class="size-3.5 text-blue-400"
                    :aria-label="$t('settings.admin.updater.table.newestFirst')"
                  />
                </span>
              </th>
              <th scope="col" class="px-3 py-3.5 font-semibold">
                {{ $t("settings.admin.updater.table.status") }}
              </th>
              <th scope="col" class="relative py-3.5 pl-3 pr-4 sm:pr-6">
                <span class="sr-only">{{
                  $t("settings.admin.updater.table.open")
                }}</span>
              </th>
            </tr>
          </thead>
          <tbody class="divide-y divide-zinc-800 text-sm tabular-nums">
            <tr
              v-for="release in data.releases"
              :key="release.id"
              class="cursor-pointer transition-colors duration-150 hover:bg-zinc-800/50"
              @click="navigateTo(`/admin/settings/updater/${release.id}`)"
            >
              <td
                class="whitespace-nowrap py-4 pl-4 pr-3 font-medium text-zinc-100 sm:pl-6"
              >
                <span class="inline-flex items-center gap-2">
                  {{ release.tag }}
                  <span
                    v-if="release.required"
                    class="rounded bg-orange-400/10 px-1.5 py-0.5 text-[11px] font-medium text-orange-300 ring-1 ring-inset ring-orange-400/20"
                    >{{ $t("settings.admin.updater.required") }}</span
                  >
                </span>
              </td>
              <td
                class="whitespace-nowrap px-3 py-4 font-mono text-xs text-zinc-500"
                :title="release.id"
              >
                {{ $t("chars.truncated", [release.id.slice(0, 8)]) }}
              </td>
              <td class="whitespace-nowrap px-3 py-4">
                <span
                  class="inline-flex items-center rounded-md bg-blue-400/10 px-2 py-1 text-xs font-medium text-blue-400 ring-1 ring-inset ring-blue-400/20"
                >
                  {{
                    $t("settings.admin.updater.platforms.appImageArch", [
                      release.arch,
                    ])
                  }}
                </span>
              </td>
              <td class="whitespace-nowrap px-3 py-4">
                <UpdaterBranchBadge :branch="release.branchSlug" />
              </td>
              <td class="whitespace-nowrap px-3 py-4 text-zinc-400">
                {{ formatReleaseSize(release.size) }}
              </td>
              <td class="whitespace-nowrap px-3 py-4 text-zinc-400">
                {{ formatReleaseDate(release.uploadedAt) }}
              </td>
              <td class="whitespace-nowrap px-3 py-4">
                <UpdaterStatusBadge :status="release.status" />
                <span
                  v-if="
                    release.status === 'offered' &&
                    release.offeredOn.length === 1
                  "
                  class="ml-1.5 text-xs text-zinc-500"
                  >{{
                    $t("settings.admin.updater.table.offeredTo", [
                      release.offeredOn[0],
                    ])
                  }}</span
                >
              </td>
              <td class="py-4 pl-3 pr-4 text-right sm:pr-6">
                <NuxtLink
                  :to="`/admin/settings/updater/${release.id}`"
                  class="text-zinc-500 hover:text-zinc-200"
                  @click.stop
                >
                  <ChevronRightIcon class="size-5" aria-hidden="true" />
                  <span class="sr-only">{{
                    $t("settings.admin.updater.table.openRelease", [
                      release.tag,
                    ])
                  }}</span>
                </NuxtLink>
              </td>
            </tr>
            <tr v-if="data.releases.length === 0">
              <td colspan="8" class="py-8 text-center text-sm text-zinc-400">
                {{ $t("settings.admin.updater.table.empty") }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { CloudArrowUpIcon } from "@heroicons/vue/24/outline";
import { ArrowDownIcon, ChevronRightIcon } from "@heroicons/vue/20/solid";
import { NuxtLink } from "#components";
import type { ClientReleaseTarget } from "~/prisma/client/enums";

definePageMeta({
  layout: "admin",
});

const { t } = useI18n();

useHead({ title: t("settings.admin.updater.pageTitle") });

const targetLabels = computed<Record<ClientReleaseTarget, string>>(() => ({
  LinuxAppImage: t("settings.admin.updater.platforms.linuxAppImage"),
}));

// Placeholders so the layout has room for platforms still to come
const plannedTargets = computed(() => [
  t("settings.admin.updater.platforms.windowsInstaller"),
]);

const data = await $dropFetch("/api/v1/admin/updater");
</script>
