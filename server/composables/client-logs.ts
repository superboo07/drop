import type { ClientLogTriggerSlug } from "~/utils/clientLogs";

export function useClientLogTriggerLabels() {
  const { t } = useI18n();
  return computed<Record<ClientLogTriggerSlug, string>>(() => ({
    launch: t("clients.admin.logs.triggers.launch"),
    crash: t("clients.admin.logs.triggers.crash"),
    manual: t("clients.admin.logs.triggers.manual"),
  }));
}
