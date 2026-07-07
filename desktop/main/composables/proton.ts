interface ProtonPaths {
  data: Ref<{
    autodiscovered: ProtonPath[];
    custom: ProtonPath[];
    default?: string;
  }>;
  refresh: () => Promise<void>;
}

const protonPaths = useState<ProtonPaths["data"]["value"]>(
  "proton_paths",
  undefined,
);

export const useProtonPaths = async (): Promise<ProtonPaths> => {
  const refresh = async () => {
    protonPaths.value = await invokeWithTimeout("fetch_proton_paths");
  };
  if (protonPaths.value)
    return {
      data: protonPaths,
      refresh,
    };

  await refresh();
  return {
    data: protonPaths,
    refresh,
  };
};
