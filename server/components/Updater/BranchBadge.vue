<template>
  <span
    :class="[
      clientReleaseBranchStyles[props.branch].class,
      'inline-flex items-center rounded-md px-2 py-1 text-xs font-medium ring-1 ring-inset',
    ]"
  >
    {{ label }}
  </span>
</template>

<script setup lang="ts">
const props = defineProps<{ branch: ClientReleaseBranchSlug }>();

const { t } = useI18n();
const label = computed(
  () =>
    ({
      release: t("settings.admin.updater.branches.release.label"),
      test: t("settings.admin.updater.branches.test.label"),
    })[props.branch],
);
</script>
