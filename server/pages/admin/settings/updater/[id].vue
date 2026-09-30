<template>
  <div class="w-full">
    <NuxtLink
      to="/admin/settings/updater"
      class="inline-flex items-center gap-2 text-sm text-zinc-400 hover:text-zinc-200"
    >
      <ArrowLeftIcon class="size-4" aria-hidden="true" />
      Client updates
    </NuxtLink>

    <div class="mt-2 flex flex-wrap items-center justify-between gap-4">
      <div class="flex flex-wrap items-center gap-3">
        <h2
          class="text-xl font-semibold tracking-tight text-zinc-100 sm:text-3xl"
        >
          Drop {{ release.tag }}
        </h2>
        <span
          class="inline-flex items-center rounded-md bg-blue-400/10 px-2 py-1 text-xs font-medium text-blue-400 ring-1 ring-inset ring-blue-400/20"
        >
          AppImage · {{ release.arch }}
        </span>
        <UpdaterBranchBadge :branch="release.branchSlug" />
        <UpdaterStatusBadge :status="release.status" />
        <span
          v-if="release.required"
          class="inline-flex items-center rounded-md bg-orange-400/10 px-2 py-1 text-xs font-medium text-orange-300 ring-1 ring-inset ring-orange-400/20"
          >Required</span
        >
      </div>
      <div class="flex gap-3">
        <LoadingButton
          v-if="release.status === 'draft'"
          :loading="busy === 'publish'"
          :disabled="!!busy"
          class="w-fit"
          @click="() => patch('publish', { publish: true })"
        >
          Publish
        </LoadingButton>
        <a
          :href="`/api/v1/admin/updater/${release.id}/download`"
          class="inline-flex items-center gap-x-2 rounded-md bg-zinc-800 px-3 py-2 text-sm font-semibold text-zinc-100 ring-1 ring-zinc-700 hover:bg-zinc-700"
        >
          <ArrowDownTrayIcon class="size-4" aria-hidden="true" />
          Download
        </a>
      </div>
    </div>

    <div class="mt-8 grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)]">
      <div class="flex flex-col gap-8">
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
              :class="[
                row.mono ? 'font-mono text-xs' : 'tabular-nums',
                'min-w-0 break-all text-right text-zinc-100',
              ]"
            >
              {{ row.value }}
              <button
                v-if="row.copy"
                type="button"
                class="ml-1 font-sans text-blue-400 hover:text-blue-300"
                @click="copy(row.value)"
              >
                Copy
              </button>
            </dd>
          </div>
        </dl>

        <!-- settings -->
        <div class="flex flex-col gap-4 rounded-xl border border-zinc-800 p-4">
          <UpdaterBranchPicker
            :model-value="release.branchSlug"
            @update:model-value="(value) => patch('branch', { branch: value })"
          />
          <div>
            <label
              for="release-tag"
              class="block text-sm font-medium text-zinc-100"
              >Tag</label
            >
            <div class="mt-2 flex gap-2">
              <input
                id="release-tag"
                v-model="tag"
                type="text"
                maxlength="64"
                class="block w-full rounded-md border-0 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 ring-1 ring-inset ring-zinc-700 focus:ring-2 focus:ring-blue-500"
              />
              <button
                type="button"
                :disabled="!!busy || !tag.trim() || tag.trim() === release.tag"
                class="rounded-md bg-zinc-800 px-3 py-2 text-sm font-semibold text-zinc-100 ring-1 ring-zinc-700 hover:bg-zinc-700 disabled:opacity-50"
                @click="() => patch('tag', { tag })"
              >
                Save
              </button>
            </div>
          </div>
          <div class="flex items-start justify-between gap-4">
            <div>
              <div class="text-sm font-medium text-zinc-100">
                Required update
              </div>
              <div class="text-xs text-zinc-400">
                Clients can't dismiss the update prompt for this release.
              </div>
            </div>
            <button
              type="button"
              role="switch"
              :aria-checked="release.required"
              :disabled="!!busy"
              :class="[
                release.required ? 'bg-blue-600' : 'bg-zinc-700',
                'relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full transition-colors',
              ]"
              @click="() => patch('required', { required: !release.required })"
            >
              <span class="sr-only">Required update</span>
              <span
                :class="[
                  release.required ? 'translate-x-5' : 'translate-x-0',
                  'pointer-events-none mt-0.5 ml-0.5 inline-block size-5 rounded-full bg-white shadow transition',
                ]"
              />
            </button>
          </div>
        </div>

        <!-- danger zone -->
        <div
          class="flex flex-col gap-4 rounded-xl border border-red-500/30 p-4"
        >
          <div
            v-if="release.status !== 'draft'"
            class="flex items-center justify-between gap-4"
          >
            <div>
              <div class="text-sm font-medium text-zinc-100">
                {{ release.withdrawnAt ? "Restore" : "Withdraw" }}
              </div>
              <div class="text-xs text-zinc-400">
                {{ withdrawHint }}
              </div>
            </div>
            <button
              type="button"
              :disabled="!!busy"
              class="shrink-0 rounded-md bg-red-400/10 px-2.5 py-1.5 text-xs font-medium text-red-400 ring-1 ring-inset ring-red-400/20 hover:bg-red-400/20"
              @click="
                () => patch('withdraw', { withdrawn: !release.withdrawnAt })
              "
            >
              {{ release.withdrawnAt ? "Restore" : "Withdraw" }}
            </button>
          </div>
          <div class="flex items-center justify-between gap-4">
            <div>
              <div class="text-sm font-medium text-zinc-100">Delete</div>
              <div class="text-xs text-zinc-400">
                Remove the file and its notes permanently. Clients that already
                installed it keep working.
              </div>
            </div>
            <button
              type="button"
              :disabled="!!busy"
              class="shrink-0 rounded-md bg-red-600 px-2.5 py-1.5 text-xs font-semibold text-white hover:bg-red-500"
              @click="confirmDelete"
            >
              Delete
            </button>
          </div>
        </div>
      </div>

      <!-- notes -->
      <div>
        <template v-if="editingNotes">
          <UpdaterNotesEditor v-model="notes" />
          <div class="mt-4 flex justify-end gap-3">
            <button
              type="button"
              class="rounded-md px-3 py-2 text-sm font-semibold text-zinc-300 hover:bg-zinc-800"
              @click="cancelNotes"
            >
              Cancel
            </button>
            <LoadingButton
              :loading="busy === 'notes'"
              :disabled="!!busy"
              class="w-fit"
              @click="saveNotes"
            >
              Save notes
            </LoadingButton>
          </div>
        </template>
        <template v-else>
          <div class="flex items-center justify-between">
            <h3 class="text-sm font-semibold text-zinc-100">Patch notes</h3>
            <button
              type="button"
              class="text-sm text-blue-400 hover:text-blue-300"
              @click="editingNotes = true"
            >
              Edit
            </button>
          </div>
          <div class="mt-3 rounded-xl bg-zinc-950 p-5 ring-1 ring-zinc-800">
            <MarkdownContent
              v-if="release.notes"
              class="prose prose-sm prose-invert prose-blue max-w-none"
              :source="release.notes"
            />
            <p v-else class="text-sm text-zinc-500">No patch notes.</p>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ArrowDownTrayIcon, ArrowLeftIcon } from "@heroicons/vue/24/outline";

definePageMeta({
  layout: "admin",
});

const route = useRoute();
const id = route.params.id!.toString();

const release = ref(
  await $dropFetch("/api/v1/admin/updater/:id", { params: { id } }),
);
useHead({ title: () => `Drop ${release.value.tag}` });

const tag = ref(release.value.tag);
const notes = ref(release.value.notes);
const editingNotes = ref(false);
const busy = ref<string | undefined>();

const facts = computed(() => [
  { label: "Release ID", value: release.value.id, mono: true, copy: true },
  {
    label: "Uploaded",
    value: formatReleaseDate(release.value.uploadedAt),
  },
  {
    label: "Published",
    value: release.value.publishedAt
      ? formatReleaseDate(release.value.publishedAt)
      : "Not yet (draft)",
  },
  ...(release.value.withdrawnAt
    ? [
        {
          label: "Withdrawn",
          value: formatReleaseDate(release.value.withdrawnAt),
        },
      ]
    : []),
  {
    label: "Uploaded by",
    value:
      release.value.uploader?.displayName ??
      (release.value.uploaderToken
        ? `API token "${release.value.uploaderToken}"`
        : "Unknown"),
  },
  { label: "File", value: release.value.fileName, mono: true },
  { label: "Size", value: formatReleaseSize(release.value.size) },
  { label: "SHA-256", value: release.value.sha256, mono: true, copy: true },
]);

const withdrawHint = computed(() => {
  if (release.value.withdrawnAt)
    return "Offer this build again. Clients get it if it's the newest published build for its platform.";
  if (release.value.status !== "offered")
    return "Stop this build from ever being offered again.";
  const branchClients =
    release.value.branchSlug === "test" ? "Test-branch clients" : "Clients";
  return release.value.fallback
    ? `Stop offering this build. ${branchClients} get ${release.value.fallback.tag} instead, including ones already on this build.`
    : `Stop offering this build. Nothing else is published for this platform, so ${branchClients.toLowerCase()} won't be offered anything.`;
});

async function refresh() {
  release.value = await $dropFetch("/api/v1/admin/updater/:id", {
    params: { id },
  });
  tag.value = release.value.tag;
}

async function patch(
  action: string,
  body: {
    tag?: string;
    notes?: string;
    required?: boolean;
    branch?: ClientReleaseBranchSlug;
    publish?: true;
    withdrawn?: boolean;
  },
) {
  busy.value = action;
  try {
    await $dropFetch("/api/v1/admin/updater/:id", {
      method: "PATCH",
      params: { id },
      body,
      failTitle: "Couldn't update the release",
    });
    await refresh();
  } catch {
    /* failTitle already showed the error */
  } finally {
    busy.value = undefined;
  }
}

async function saveNotes() {
  await patch("notes", { notes: notes.value });
  editingNotes.value = false;
}

function cancelNotes() {
  notes.value = release.value.notes;
  editingNotes.value = false;
}

function copy(value: string) {
  navigator.clipboard.writeText(value).catch(() => {});
}

function confirmDelete() {
  createModal(
    ModalType.Confirmation,
    {
      title: `Delete Drop ${release.value.tag}?`,
      description:
        "The file and its patch notes are removed from the server. This can't be undone. To stop offering it but keep the record, withdraw it instead.",
      buttonText: "Delete",
    },
    async (event, close) => {
      if (event !== "confirm") return close();
      try {
        await $dropFetch("/api/v1/admin/updater/:id", {
          method: "DELETE",
          params: { id },
          failTitle: "Couldn't delete the release",
        });
      } catch {
        return close();
      }
      close();
      await navigateTo("/admin/settings/updater");
    },
  );
}
</script>
