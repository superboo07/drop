<template>
  <div class="w-full">
    <NuxtLink
      to="/admin/settings/updater"
      class="inline-flex items-center gap-2 text-sm text-zinc-400 hover:text-zinc-200"
    >
      <ArrowLeftIcon class="size-4" aria-hidden="true" />
      Client updates
    </NuxtLink>
    <h2
      class="mt-2 text-xl font-semibold tracking-tight text-zinc-100 sm:text-3xl"
    >
      Upload release
    </h2>

    <form
      class="mt-8 grid gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]"
      @submit.prevent="() => submit(true)"
    >
      <div class="flex flex-col gap-6">
        <!-- file -->
        <div>
          <span class="block text-sm font-medium text-zinc-100"
            >Build file</span
          >
          <label
            v-if="!file"
            for="release-file"
            :class="[
              dragging
                ? 'border-blue-400 bg-blue-400/5'
                : 'border-zinc-700 hover:border-zinc-500',
              'mt-2 flex cursor-pointer flex-col items-center justify-center gap-2 rounded-lg border-2 border-dashed px-6 py-10 text-center transition-colors',
            ]"
            @dragover.prevent="dragging = true"
            @dragleave.prevent="dragging = false"
            @drop.prevent="onDrop"
          >
            <CloudArrowUpIcon class="size-8 text-zinc-500" aria-hidden="true" />
            <span class="text-sm text-zinc-300">
              Drop a file here or
              <span class="font-semibold text-blue-400">choose one</span>
            </span>
            <span class="text-xs text-zinc-500">.AppImage for Linux</span>
            <input
              id="release-file"
              type="file"
              accept=".AppImage"
              class="sr-only"
              @change="onPick"
            />
          </label>
          <div
            v-else
            class="mt-2 rounded-lg border border-zinc-700 bg-zinc-950 p-4"
          >
            <div class="flex items-center gap-3">
              <DocumentIcon
                class="size-8 shrink-0 text-zinc-500"
                aria-hidden="true"
              />
              <div class="min-w-0 grow">
                <div class="truncate font-mono text-sm text-zinc-100">
                  {{ file.name }}
                </div>
                <div class="text-xs text-zinc-500 tabular-nums">
                  <template v-if="upload">
                    {{ formatReleaseSize(upload.size) }} · SHA-256
                    {{ upload.sha256.slice(0, 12) }}…
                  </template>
                  <template v-else-if="uploadError">{{ uploadError }}</template>
                  <template v-else-if="progress >= 1">
                    Checking the file…
                  </template>
                  <template v-else>
                    {{ formatReleaseSize(file.size * progress) }} of
                    {{ formatReleaseSize(file.size) }}
                  </template>
                </div>
              </div>
              <CheckCircleIcon
                v-if="upload"
                class="size-5 text-green-400"
                aria-hidden="true"
              />
              <button
                type="button"
                class="text-zinc-500 hover:text-zinc-200"
                @click="clearFile"
              >
                <XMarkIcon class="size-5" aria-hidden="true" />
                <span class="sr-only">Remove file</span>
              </button>
            </div>
            <div
              v-if="!upload && !uploadError"
              class="mt-3 h-1.5 overflow-hidden rounded-full bg-zinc-800"
            >
              <div
                class="h-full rounded-full bg-blue-500 transition-[width]"
                :style="{ width: `${Math.round(progress * 100)}%` }"
              />
            </div>
          </div>
          <p v-if="uploadError" class="mt-2 text-sm text-red-400">
            {{ uploadError }}
          </p>
        </div>

        <!-- platform -->
        <div>
          <label
            for="release-platform"
            class="block text-sm font-medium text-zinc-100"
            >Platform</label
          >
          <select
            id="release-platform"
            v-model="target"
            class="mt-2 block w-full rounded-md border-0 bg-zinc-800 py-2 pl-3 pr-10 text-sm text-zinc-100 ring-1 ring-inset ring-zinc-700 focus:ring-2 focus:ring-blue-500"
          >
            <option value="linux-appimage">Linux · AppImage</option>
            <option disabled>Windows · Installer (not supported yet)</option>
          </select>
        </div>

        <!-- arch -->
        <div>
          <span class="block text-sm font-medium text-zinc-100"
            >Architecture</span
          >
          <div
            class="mt-2 inline-flex overflow-hidden rounded-md text-sm ring-1 ring-zinc-700"
          >
            <button
              v-for="option in ['x86_64', 'aarch64'] as const"
              :key="option"
              type="button"
              :disabled="!!upload && upload.arch !== option"
              :class="[
                arch === option
                  ? 'bg-blue-600 font-medium text-white'
                  : 'text-zinc-400 hover:bg-zinc-800 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent',
                'px-4 py-2',
              ]"
              @click="arch = option"
            >
              {{ option }}
            </button>
          </div>
          <p v-if="upload" class="mt-1 text-xs text-zinc-500">
            Detected from the file: {{ upload.arch }}.
          </p>
        </div>

        <div>
          <UpdaterBranchPicker v-model="branch" />
          <p class="mt-1 text-xs text-zinc-500">
            Builds whose version contains “.dirty” (made with uncommitted
            changes) are put on Test automatically.
          </p>
        </div>

        <!-- tag -->
        <div>
          <label
            for="release-tag"
            class="block text-sm font-medium text-zinc-100"
            >Tag</label
          >
          <input
            id="release-tag"
            v-model="tag"
            type="text"
            maxlength="64"
            placeholder="0.4.3"
            class="mt-2 block w-full rounded-md border-0 bg-zinc-800 px-3 py-2 text-sm text-zinc-100 ring-1 ring-inset ring-zinc-700 placeholder:text-zinc-500 focus:ring-2 focus:ring-blue-500 tabular-nums"
          />
          <p class="mt-1 text-xs text-zinc-500">
            Shown to players. It doesn't have to be unique: you can reuse the
            tag of a withdrawn or deleted build. Every upload gets its own
            release ID.
          </p>
        </div>

        <!-- required -->
        <div class="flex gap-3">
          <input
            id="release-required"
            v-model="required"
            type="checkbox"
            class="mt-0.5 size-4 rounded border-zinc-600 bg-zinc-800 text-blue-600 focus:ring-blue-500"
          />
          <label for="release-required" class="text-sm">
            <span class="font-medium text-zinc-100">Required update</span>
            <span class="block text-xs text-zinc-500">
              Clients can't dismiss the update prompt. Use this when older
              clients no longer work with the server.
            </span>
          </label>
        </div>
      </div>

      <UpdaterNotesEditor v-model="notes" />

      <div
        class="flex flex-wrap items-center justify-end gap-3 border-t border-zinc-800 pt-6 lg:col-span-2"
      >
        <p v-if="submitError" class="mr-auto text-sm text-red-400">
          {{ submitError }}
        </p>
        <NuxtLink
          to="/admin/settings/updater"
          class="rounded-md px-3 py-2 text-sm font-semibold text-zinc-300 hover:bg-zinc-800"
          >Cancel</NuxtLink
        >
        <button
          type="button"
          :disabled="!ready || submitting"
          class="rounded-md bg-zinc-800 px-3 py-2 text-sm font-semibold text-zinc-100 ring-1 ring-zinc-700 hover:bg-zinc-700 disabled:cursor-not-allowed disabled:opacity-50"
          @click="() => submit(false)"
        >
          Save as draft
        </button>
        <LoadingButton
          :loading="submitting"
          :disabled="!ready || submitting"
          class="w-fit"
        >
          Publish {{ tag }}
        </LoadingButton>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import {
  ArrowLeftIcon,
  CloudArrowUpIcon,
  DocumentIcon,
} from "@heroicons/vue/24/outline";
import { CheckCircleIcon, XMarkIcon } from "@heroicons/vue/20/solid";

definePageMeta({
  layout: "admin",
});

useHead({ title: "Upload release" });

type UploadResult = {
  uploadId: string;
  size: number;
  sha256: string;
  arch: "x86_64" | "aarch64";
  suggestedTag?: string;
  suggestedBranch: ClientReleaseBranchSlug;
};

const target = ref("linux-appimage");
const arch = ref<"x86_64" | "aarch64">("x86_64");
const branch = ref<ClientReleaseBranchSlug>("release");
const tag = ref("");
const notes = ref("");
const required = ref(false);

const dragging = ref(false);
const file = ref<File | undefined>();
const progress = ref(0);
const upload = ref<UploadResult | undefined>();
const uploadError = ref<string | undefined>();
let request: XMLHttpRequest | undefined;

const submitting = ref(false);
const submitError = ref<string | undefined>();

const ready = computed(() => !!upload.value && tag.value.trim().length > 0);

function onPick(event: Event) {
  const picked = (event.target as HTMLInputElement).files?.[0];
  if (picked) startUpload(picked);
}

function onDrop(event: DragEvent) {
  dragging.value = false;
  const dropped = event.dataTransfer?.files?.[0];
  if (dropped) startUpload(dropped);
}

// XHR rather than fetch: fetch can't report upload progress
function startUpload(picked: File) {
  clearFile();
  file.value = picked;

  const xhr = new XMLHttpRequest();
  request = xhr;
  const params = new URLSearchParams({
    target: target.value,
    fileName: picked.name,
  });
  xhr.open("PUT", `/api/v1/admin/updater/upload?${params}`);
  xhr.setRequestHeader("Content-Type", "application/octet-stream");
  xhr.upload.onprogress = (e) => {
    if (e.lengthComputable) progress.value = e.loaded / e.total;
  };
  xhr.onload = () => {
    if (request !== xhr) return;
    let body: Record<string, unknown> = {};
    try {
      body = JSON.parse(xhr.responseText);
    } catch {
      /* non-JSON error page */
    }
    if (xhr.status >= 200 && xhr.status < 300) {
      const result = body as UploadResult;
      upload.value = result;
      arch.value = result.arch;
      branch.value = result.suggestedBranch;
      if (!tag.value && result.suggestedTag) tag.value = result.suggestedTag;
    } else {
      uploadError.value =
        (body.message as string) ||
        (body.statusMessage as string) ||
        `Upload failed (HTTP ${xhr.status}).`;
    }
  };
  xhr.onerror = () => {
    if (request !== xhr) return;
    uploadError.value = "Upload failed. Check your connection and try again.";
  };
  xhr.send(picked);
}

function clearFile() {
  request?.abort();
  request = undefined;
  if (upload.value) discardUpload(upload.value.uploadId);
  file.value = undefined;
  upload.value = undefined;
  uploadError.value = undefined;
  progress.value = 0;
}

function discardUpload(uploadId: string) {
  $dropFetch("/api/v1/admin/updater/upload/:id", {
    method: "DELETE",
    params: { id: uploadId },
  }).catch(() => {});
}

async function submit(publish: boolean) {
  if (!upload.value || !ready.value) return;
  submitting.value = true;
  submitError.value = undefined;
  try {
    const release = await $dropFetch("/api/v1/admin/updater", {
      method: "POST",
      body: {
        uploadId: upload.value.uploadId,
        tag: tag.value,
        arch: arch.value,
        branch: branch.value,
        notes: notes.value,
        required: required.value,
        publish,
      },
    });
    upload.value = undefined; // now owned by the release
    await navigateTo(`/admin/settings/updater/${release.id}`);
  } catch (e) {
    submitError.value =
      // @ts-expect-error h3 errors carry the message here
      e?.data?.message ?? e?.statusMessage ?? "Couldn't save the release.";
  } finally {
    submitting.value = false;
  }
}

onBeforeUnmount(() => {
  request?.abort();
  if (upload.value) discardUpload(upload.value.uploadId);
});
</script>
