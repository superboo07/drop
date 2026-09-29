import { listen } from "@tauri-apps/api/event";
import {
  AppStatus,
  type ClientUpdateProgress,
  type ClientUpdateStatus,
  type Settings,
} from "~/types";

export const useClientUpdateStatus = () =>
  useState<ClientUpdateStatus | undefined>("client-update-status");

export const useClientUpdateProgress = () =>
  useState<ClientUpdateProgress | undefined>("client-update-progress");

/**
 * Whether the player should be told about the update found: set by a
 * start-up check (unless they turned those off and it isn't required) or by
 * checking by hand
 */
export const useClientUpdateAnnounced = () =>
  useState<boolean>("client-update-announced", () => false);

/** Whether the update dialog is open */
export const useClientUpdateDialog = () =>
  useState<boolean>("client-update-dialog", () => false);

listen<ClientUpdateStatus>("client_update/status", (event) => {
  useClientUpdateStatus().value = event.payload;
});

listen<ClientUpdateProgress>("client_update/progress", (event) => {
  useClientUpdateProgress().value = event.payload;
});

export async function checkForClientUpdate(manual = true) {
  const status = useClientUpdateStatus();
  status.value = await invokeWithTimeout<ClientUpdateStatus>(
    "check_client_update",
    undefined,
    // Hashing a ~150MB AppImage on a slow disk plus a slow server
    120_000,
  );
  if (manual) useClientUpdateAnnounced().value = true;
  return status.value;
}

export async function installClientUpdate() {
  useClientUpdateProgress().value = undefined;
  // Runs for as long as the download takes; progress arrives as events
  await invokeWithTimeout("install_client_update", undefined, Infinity);
}

export async function cancelClientUpdate() {
  await invokeWithTimeout("cancel_client_update");
}

export async function restartAfterClientUpdate() {
  await invokeWithTimeout("restart_after_client_update");
}

/**
 * Checks once the client is signed in, and opens the dialog if something is
 * waiting: always for a required update, otherwise only when the player
 * hasn't turned off start-up checks.
 */
export function setupClientUpdateChecks() {
  const appState = useAppState();
  let checked = false;

  watch(
    () => appState.value?.status,
    async (status) => {
      if (checked || status !== AppStatus.SignedIn) return;
      checked = true;

      useClientUpdateStatus().value =
        await invokeWithTimeout<ClientUpdateStatus>(
          "fetch_client_update_status",
        );

      let result: ClientUpdateStatus;
      try {
        result = await checkForClientUpdate(false);
      } catch (e) {
        console.warn("client update check failed", e);
        return;
      }
      if (!result.update) return;

      const settings = await invokeWithTimeout<Settings>("fetch_settings");
      if (result.update.required || settings.checkUpdatesOnStart) {
        useClientUpdateAnnounced().value = true;
        useClientUpdateDialog().value = true;
      }
    },
    { immediate: true },
  );
}

/** The header widget shows while an announced update waits, or after installing */
export const useShowClientUpdateWidget = () => {
  const status = useClientUpdateStatus();
  const announced = useClientUpdateAnnounced();
  return computed(
    () =>
      !!status.value?.installed ||
      (!!status.value?.update && announced.value),
  );
};
