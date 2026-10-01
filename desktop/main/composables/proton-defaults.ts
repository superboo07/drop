import type { InjectionKey } from "vue";
import type { GameVersion, ProtonDefaults } from "~/types";

type UserConfiguration = GameVersion["userConfiguration"];

// Provided by the game options modal to every tab, so each setting can show
// whether it follows the server's recommendation.
export const PROTON_DEFAULTS_KEY: InjectionKey<ProtonDefaults | null> =
  Symbol("protonDefaults");

export type RecommendationState =
  | { state: "recommended" }
  | { state: "changed"; server: string }
  | undefined;

// Compares a setting with the server's recommendation: undefined when the
// server has none, otherwise whether the game still uses it.
export function recommendationState<T>(
  recommended: T | null | undefined,
  current: T,
  describe: (value: T) => string,
): RecommendationState {
  if (recommended === null || recommended === undefined) return undefined;
  return recommended === current
    ? { state: "recommended" }
    : { state: "changed", server: describe(recommended) };
}

// The server says whether DXVK/Esync/Fsync should be on; the configuration
// stores whether they're disabled.
export function disabledFromRecommendation(on: boolean | null) {
  return on === null ? null : !on;
}

// Mirrors the client's Proton matching (client::proton): a build matches a
// name when its display or folder name contains it, case-insensitively.
export function protonMatchesName(
  proton: { name: string; path: string },
  name: string,
) {
  const needle = name.trim().toLowerCase();
  if (!needle) return false;
  const folder = proton.path.split("/").filter(Boolean).pop() ?? "";
  return [proton.name, folder].some((v) => v.toLowerCase().includes(needle));
}

// Sets every setting the server has a recommendation for back to it. The
// preferred Proton is filled in with the installed build closest to the
// recommended name, as at install; it's left alone if none matches.
export async function resetToRecommended(
  configuration: UserConfiguration,
  defaults: ProtonDefaults,
) {
  const dxvk = disabledFromRecommendation(defaults.dxvk);
  const esync = disabledFromRecommendation(defaults.esync);
  const fsync = disabledFromRecommendation(defaults.fsync);
  if (dxvk !== null) configuration.disableDxvk = dxvk;
  if (esync !== null) configuration.disableEsync = esync;
  if (fsync !== null) configuration.disableFsync = fsync;
  if (defaults.locale !== null) configuration.locale = defaults.locale;
  if (defaults.extraEnvVars !== null)
    configuration.extraEnvVars = defaults.extraEnvVars;
  if (defaults.protonName) {
    const match = await matchProtonName(defaults.protonName);
    if (match) configuration.overrideProtonPath = match.path;
  }
}

// The installed Proton build a recommended name fills in as. undefined where
// there's no Proton (the command only exists on Linux) or nothing matches.
export async function matchProtonName(name: string) {
  try {
    return (
      (await invokeWithTimeout<{ name: string; path: string } | null>(
        "match_proton_name",
        { name },
      )) ?? undefined
    );
  } catch {
    return undefined;
  }
}
