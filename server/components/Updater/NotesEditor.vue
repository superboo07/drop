<template>
  <div class="flex flex-col">
    <div class="flex items-end justify-between">
      <label :for="id" class="block text-sm font-medium text-zinc-100">
        {{ $t("settings.admin.updater.notes.label") }}
      </label>
      <div class="flex gap-1 text-xs">
        <button
          v-for="mode in ['Write', 'Preview'] as const"
          :key="mode"
          type="button"
          :class="[
            tab === mode
              ? 'bg-zinc-800 text-zinc-100'
              : 'text-zinc-400 hover:text-zinc-200',
            'rounded-md px-2.5 py-1 font-medium',
          ]"
          @click="tab = mode"
        >
          {{ modeLabels[mode] }}
        </button>
      </div>
    </div>
    <textarea
      v-if="tab === 'Write'"
      :id="id"
      v-model="model"
      rows="14"
      :placeholder="$t('settings.admin.updater.notes.placeholder')"
      class="mt-2 block w-full grow rounded-lg bg-zinc-950 px-4 py-3 font-mono text-sm leading-6 text-zinc-300 ring-1 ring-inset ring-zinc-700 placeholder:text-zinc-600 focus:ring-2 focus:ring-blue-500 focus:outline-none border-0"
    />
    <div
      v-else
      class="mt-2 min-h-[21rem] grow rounded-lg bg-zinc-950 px-4 py-3 ring-1 ring-inset ring-zinc-700"
    >
      <MarkdownContent
        v-if="model"
        class="prose prose-sm prose-invert prose-blue"
        :source="model"
      />
      <p v-else class="text-sm text-zinc-500">
        {{ $t("settings.admin.updater.notes.nothingToPreview") }}
      </p>
    </div>
    <p class="mt-2 text-xs text-zinc-500">
      {{ $t("settings.admin.updater.notes.hint") }}
    </p>
  </div>
</template>

<script setup lang="ts">
const model = defineModel<string>({ required: true });
const id = useId();
const tab = ref<"Write" | "Preview">("Write");
const { t } = useI18n();
const modeLabels = computed(() => ({
  Write: t("settings.admin.updater.notes.write"),
  Preview: t("settings.admin.updater.notes.preview"),
}));
</script>
