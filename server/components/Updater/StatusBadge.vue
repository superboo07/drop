<template>
  <span
    :class="[
      clientReleaseStatusStyles[props.status].class,
      'inline-flex items-center rounded-md px-2 py-1 text-xs font-medium ring-1 ring-inset',
    ]"
  >
    {{ label }}
  </span>
</template>

<script setup lang="ts">
const props = defineProps<{ status: ClientReleaseStatus }>();

const { t } = useI18n();
const label = computed(
  () =>
    ({
      draft: t("settings.admin.updater.status.draft"),
      offered: t("settings.admin.updater.status.offered"),
      superseded: t("settings.admin.updater.status.superseded"),
      withdrawn: t("settings.admin.updater.status.withdrawn"),
    })[props.status],
);
</script>
