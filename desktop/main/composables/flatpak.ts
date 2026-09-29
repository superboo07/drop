export type FlatpakApp = { id: string; name: string; branches: string[] };

const apps = ref<FlatpakApp[] | undefined>();
let loading: Promise<void> | undefined;

// Installed Flatpak apps, fetched once (on first use) and shared.
export const useFlatpakApps = () => {
  const load = () => {
    loading ??= invokeWithTimeout<FlatpakApp[]>("fetch_flatpak_apps")
      .then((result) => {
        apps.value = result;
      })
      .catch((e) => {
        console.error("failed to list flatpak apps", e);
        apps.value = [];
      });
    return loading;
  };
  const nameOf = (appId: string) =>
    apps.value?.find((app) => app.id === appId)?.name;
  return { apps, load, nameOf };
};
