<template>
  <fieldset>
    <legend class="block text-sm font-medium text-zinc-100">
      {{ $t("settings.admin.updater.branches.legend") }}
    </legend>
    <div class="mt-2 grid gap-3 sm:grid-cols-2">
      <label
        v-for="option in options"
        :key="option.value"
        :class="[
          model === option.value
            ? 'bg-zinc-800/60 ring-2 ring-blue-500'
            : 'bg-zinc-800/30 ring-1 ring-zinc-700 hover:bg-zinc-800/50',
          'flex cursor-pointer gap-3 rounded-lg p-3',
        ]"
      >
        <input
          v-model="model"
          type="radio"
          :value="option.value"
          :name="name"
          class="mt-0.5 size-4 border-zinc-600 bg-zinc-800 text-blue-600 focus:ring-blue-500"
        />
        <span>
          <span class="block text-sm font-medium text-zinc-100">{{
            option.label
          }}</span>
          <span class="mt-0.5 block text-xs text-zinc-400">{{
            option.description
          }}</span>
        </span>
      </label>
    </div>
  </fieldset>
</template>

<script setup lang="ts">
const model = defineModel<ClientReleaseBranchSlug>({ required: true });
const name = useId();

const { t } = useI18n();
const options = computed(() => [
  {
    value: "release" as const,
    label: t("settings.admin.updater.branches.release.label"),
    description: t("settings.admin.updater.branches.release.description"),
  },
  {
    value: "test" as const,
    label: t("settings.admin.updater.branches.test.label"),
    description: t("settings.admin.updater.branches.test.description"),
  },
]);
</script>
