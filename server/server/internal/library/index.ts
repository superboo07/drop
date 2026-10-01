/**
 * The Library Manager keeps track of games in Drop's library and their various states.
 * It uses path relative to the library, so it can moved without issue
 *
 * It also provides the endpoints with information about unmatched games
 */

import path from "path";
import prisma from "../db/database";
import { fuzzy } from "fast-fuzzy";
import type { TaskRunContext } from "../tasks";
import taskHandler from "../tasks";
import notificationSystem from "../notifications";
import { GameNotFoundError, type LibraryProvider } from "./provider";
import { logger } from "../logging";
import type { GameModel } from "~/prisma/client/models";
import { createHash } from "node:crypto";
import type { WorkingLibrarySource } from "~/server/api/v1/admin/library/sources/index.get";
import gameSizeManager from "~/server/internal/gamesize";
import type { ImportVersion } from "~/server/api/v1/admin/import/version/index.post";
import { GameType, type Platform } from "~/prisma/client/enums";
import { castManifest } from "./manifest/utils";
import {
  fetchDeltaDependents,
  fetchUpdateFamily,
  invalidateManifestCache,
  resolveBaseChain,
} from "./manifest";
import TORRENTIAL_SERVICE from "~/server/internal/services/torrential";
import { Shescape } from "shescape";
import type { Prisma } from "~/prisma/client/client";

export function createGameImportTaskId(libraryId: string, libraryPath: string) {
  return createHash("md5")
    .update(`import:${libraryId}:${libraryPath}`)
    .digest("hex");
}

export function createVersionImportTaskKey(
  gameId: string,
  versionName: string,
) {
  return createHash("md5")
    .update(`import:${gameId}:${versionName}`)
    .digest("hex");
}

export function createVersionResyncTaskKey(gameId: string, versionId: string) {
  return createHash("md5")
    .update(`resync:${gameId}:${versionId}`)
    .digest("hex");
}

type VersionLaunchInput = (typeof ImportVersion.infer)["launches"][number];
type VersionSetupInput = (typeof ImportVersion.infer)["setups"][number];

/**
 * Shapes launch-config input into Prisma createMany data, shared between
 * importVersion and the config-update route so the emulator/emulatorSuggestions
 * field logic can't drift between the two.
 */
export function buildLaunchCreateData(
  launches: VersionLaunchInput[],
  gameType: GameType,
) {
  return launches.map((v) => ({
    name: v.name,
    command: v.launch,
    platform: v.platform,
    ...(v.emulatorId && gameType === "Game"
      ? { emulatorId: v.emulatorId }
      : undefined),
    emulatorSuggestions: gameType === "Emulator" ? (v.suggestions ?? []) : [],
    umuIdOverride: v.umuId?.trim() || null,
    workingDirectory: normalizeWorkingDirectory(v.workingDirectory),
  }));
}

export function buildSetupCreateData(setups: VersionSetupInput[]) {
  return setups.map((v) => ({
    command: v.launch,
    platform: v.platform,
    workingDirectory: normalizeWorkingDirectory(v.workingDirectory),
  }));
}

/**
 * Cleans up a launch/setup working directory override into a relative,
 * forward-slashed path. Empty means no override (null); "." is the install
 * root itself. Throws on anything that would leave the install directory,
 * so both the server and the client can trust what's stored.
 */
export function normalizeWorkingDirectory(dir: string | undefined | null) {
  const trimmed = dir?.trim().replaceAll("\\", "/");
  if (!trimmed) return null;
  if (trimmed.startsWith("/") || /^[a-zA-Z]:/.test(trimmed))
    throw createError({
      statusCode: 400,
      message: `Working directory "${trimmed}" must be relative to the install folder.`,
    });
  const parts = trimmed.split("/").filter((v) => v && v !== ".");
  if (parts.includes(".."))
    throw createError({
      statusCode: 400,
      message: `Working directory "${trimmed}" can't leave the install folder.`,
    });
  return parts.length > 0 ? parts.join("/") : ".";
}

/**
 * Guard rails shared between creating a new version and editing an existing
 * one's config: a delta version needs a base version of the same game whose
 * chain reaches a full version, and setup-only versions need setups while
 * normal ones need launches. `excludeVersionId` is the version being edited,
 * which can't be its own base or build on anything built on top of it.
 */
export async function validateVersionMetadata(
  gameId: string,
  metadata: {
    delta: boolean;
    baseVersionId?: string | null;
    onlySetup: boolean;
    launches: { platform: Platform; workingDirectory?: string }[];
    setups: { platform: Platform; workingDirectory?: string }[];
  },
  excludeVersionId?: string,
) {
  for (const config of [...metadata.launches, ...metadata.setups])
    normalizeWorkingDirectory(config.workingDirectory);

  if (metadata.delta) {
    if (!metadata.baseVersionId)
      throw createError({
        statusCode: 400,
        message: "Update mode requires a base version to apply on top of.",
      });
    if (metadata.baseVersionId === excludeVersionId)
      throw createError({
        statusCode: 400,
        message: "A version can't be its own base version.",
      });

    const base = await prisma.gameVersion.findFirst({
      where: { versionId: metadata.baseVersionId, gameId },
      select: { versionId: true, delta: true, baseVersionId: true },
    });
    if (!base)
      throw createError({
        statusCode: 400,
        message: "The selected base version doesn't exist for this game.",
      });

    if (excludeVersionId) {
      const dependents = await fetchDeltaDependents(gameId, excludeVersionId);
      if (dependents.some((v) => v.versionId === base.versionId))
        throw createError({
          statusCode: 400,
          message:
            "The selected base version builds on this version, which would make a loop.",
        });
    }

    // Throws if the base's own chain is broken (missing base, loop).
    if (base.delta)
      await resolveBaseChain(base.baseVersionId, base.versionId).catch((e) => {
        throw createError({
          statusCode: 400,
          message: `The selected base version can't be resolved: ${e?.message ?? e}`,
        });
      });
  }

  if (metadata.onlySetup) {
    if (metadata.setups.length == 0)
      throw createError({
        statusCode: 400,
        message: 'Setup required in "setup mode".',
      });
  } else {
    if (metadata.launches.length == 0)
      throw createError({
        statusCode: 400,
        message: "Launch executable is required.",
      });
  }
}

export interface EmulatorVersionGuess {
  type: "emulator";
  emulatorId: string;
  icon: string;
  gameName: string;
  versionName: string;
  launchName: string;
  platform: Platform;
}
export interface PlatformVersionGuess {
  platform: Platform;
  type: "platform";
}
export type VersionGuess = {
  filename: string;
  match: number;
} & (PlatformVersionGuess | EmulatorVersionGuess);

export interface UnimportedVersionInformation {
  type: "local" | "depot";
  name: string;
  identifier: string;
}

class LibraryManager {
  private libraries: Map<string, LibraryProvider<unknown>> = new Map();
  private shescape = new Shescape({});

  addLibrary(library: LibraryProvider<unknown>) {
    this.libraries.set(library.id(), library);
  }

  removeLibrary(id: string) {
    this.libraries.delete(id);
  }

  getLibrary(libraryId: string): LibraryProvider<unknown> | undefined {
    return this.libraries.get(libraryId);
  }

  async fetchLibraries(): Promise<WorkingLibrarySource[]> {
    const libraries = await prisma.library.findMany({});

    const libraryWithMetadata = libraries.map(async (library) => {
      const theLibrary = this.libraries.get(library.id);
      const working = this.libraries.has(library.id);
      return {
        ...library,
        working,
        fsStats: working ? theLibrary?.fsStats() : undefined,
      };
    });
    return await Promise.all(libraryWithMetadata);
  }

  async fetchGamesByLibrary() {
    const results: { [key: string]: { [key: string]: GameModel } } = {};
    const games = await prisma.game.findMany({});
    for (const game of games) {
      const libraryId = game.libraryId!;
      const libraryPath = game.libraryPath!;

      results[libraryId] ??= {};
      results[libraryId][libraryPath] = game;
    }

    return results;
  }

  async fetchUnimportedGames() {
    const unimportedGames: { [key: string]: string[] } = {};
    const instanceGames = await this.fetchGamesByLibrary();

    for (const [id, library] of this.libraries.entries()) {
      const providerGames = await library.listGames();
      const providerUnimportedGames = providerGames.filter(
        (libraryPath) =>
          !instanceGames[id]?.[libraryPath] &&
          !taskHandler.hasTaskKey(createGameImportTaskId(id, libraryPath)),
      );
      unimportedGames[id] = providerUnimportedGames;
    }

    return unimportedGames;
  }

  async fetchUnimportedGameVersions(
    libraryId: string,
    libraryPath: string,
    noFetchParams?: {
      gameId: string;
      versions: string[];
      depotVersions: { id: string; versionName: string }[];
    },
  ): Promise<UnimportedVersionInformation[] | undefined> {
    const provider = this.libraries.get(libraryId);
    if (!provider) return undefined;
    let params = noFetchParams;
    if (!params) {
      const game = await prisma.game.findUnique({
        where: {
          libraryKey: {
            libraryId,
            libraryPath,
          },
        },
        select: {
          id: true,
          versions: {
            select: {
              versionPath: true,
            },
          },
        },
      });
      if (!game) return undefined;
      const depotVersions = await prisma.unimportedGameVersion.findMany({
        where: {
          gameId: game.id,
        },
        select: {
          versionName: true,
          id: true,
        },
      });

      params = {
        gameId: game.id,
        versions: game.versions
          .map((v) => v.versionPath)
          .filter((v) => v !== null),
        depotVersions: depotVersions,
      };
    }

    try {
      const versions = await provider.listVersions(
        libraryPath,
        params.versions,
      );
      const unimportedVersions = versions
        .filter(
          (e) =>
            params.versions.findIndex((v) => v == e) == -1 &&
            !taskHandler.hasTaskKey(
              createVersionImportTaskKey(params.gameId, e),
            ),
        )
        .map(
          (v) =>
            ({
              type: "local",
              name: v,
              identifier: v,
            }) satisfies UnimportedVersionInformation,
        );
      const mappedDepotVersions = params.depotVersions.map(
        (v) =>
          ({
            type: "depot",
            name: v.versionName,
            identifier: v.id,
          }) satisfies UnimportedVersionInformation,
      );
      return [...unimportedVersions, ...mappedDepotVersions];
    } catch (e) {
      if (e instanceof GameNotFoundError) {
        logger.warn(e);
        return undefined;
      }
      throw e;
    }
  }

  async fetchGamesWithStatus(
    where: Partial<Omit<Prisma.GameFindManyArgs, "include">>,
  ) {
    const games = await prisma.game.findMany({
      ...where,
      include: {
        library: true,
        versions: true,
        unimportedGameVersions: true,
      },
    });

    return await Promise.all(
      games.map(async (e) => {
        const unimportedVersions = await this.fetchUnimportedGameVersions(
          e.libraryId ?? "",
          e.libraryPath,
          {
            gameId: e.id,
            versions: e.versions
              .map((v) => v.versionPath)
              .filter((v) => v !== null),
            depotVersions: e.unimportedGameVersions,
          },
        );
        return {
          game: e,
          status: unimportedVersions
            ? {
                noVersions: e.versions.length == 0,
                unimportedVersions: unimportedVersions,
              }
            : ("offline" as const),
        };
      }),
    );
  }

  /**
   * Lists the files of a version that hasn't been imported yet - read from
   * disk for a local version, from the staged upload for a depot one.
   */
  async fetchUnimportedVersionFiles(
    gameId: string,
    versionIdentifier: Omit<UnimportedVersionInformation, "name">,
  ): Promise<string[] | undefined> {
    if (versionIdentifier.type === "depot") {
      const unimported = await prisma.unimportedGameVersion.findFirst({
        where: { id: versionIdentifier.identifier, gameId },
        select: { fileList: true },
      });
      return unimported?.fileList;
    }

    const game = await prisma.game.findUnique({
      where: { id: gameId },
      select: { libraryPath: true, libraryId: true },
    });
    if (!game || !game.libraryId) return undefined;
    const library = this.libraries.get(game.libraryId);
    if (!library) return undefined;
    return await library.versionReaddir(
      game.libraryPath,
      versionIdentifier.identifier,
    );
  }

  /**
   * Fetches recommendations and extra data about the version. Doesn't actually check if it's been imported.
   * @param gameId
   * @param versionIdentifier
   * @returns
   */
  async fetchUnimportedVersionInformation(
    gameId: string,
    versionIdentifier: Omit<UnimportedVersionInformation, "name">,
  ) {
    const game = await prisma.game.findUnique({
      where: { id: gameId },
      select: { libraryPath: true, libraryId: true, mName: true },
    });
    if (!game || !game.libraryId) return undefined;

    const library = this.libraries.get(game.libraryId);
    if (!library) return undefined;

    const fileExts: { [key in Platform]: string[] } = {
      Linux: [
        // Ext for Unity games
        ".x86_64",
        // Shell scripts
        ".sh",
        // No extension is common for Linux binaries
        "",
        // AppImages
        ".appimage",
      ],
      Windows: [".exe", ".bat"],
      macOS: [
        // App files
        ".app",
      ],
    };

    const emulators = await prisma.launchConfiguration.findMany({
      where: {
        emulatorSuggestions: {
          isEmpty: false,
        },
        gameVersion: {
          game: {
            type: GameType.Emulator,
          },
        },
      },
      select: {
        emulatorSuggestions: true,
        gameVersion: {
          select: {
            game: {
              select: {
                mIconObjectId: true,
                mName: true,
              },
            },
            displayName: true,
            versionPath: true,
          },
        },
        name: true,
        launchId: true,
        platform: true,
      },
    });

    const options: Array<VersionGuess> = [];

    const files = await this.fetchUnimportedVersionFiles(
      gameId,
      versionIdentifier,
    );
    if (!files) return undefined;

    for (const filename of files) {
      const basename = path.basename(filename);
      const dotLocation = filename.lastIndexOf(".");
      const ext =
        dotLocation == -1 ? "" : filename.slice(dotLocation).toLowerCase();
      for (const [platform, checkExts] of Object.entries(fileExts)) {
        for (const checkExt of checkExts) {
          if (checkExt != ext) continue;
          const fuzzyValue = fuzzy(basename, game.mName);
          options.push({
            type: "platform",
            filename: this.shescape.escape(filename),
            platform: platform as Platform,
            match: fuzzyValue,
          });
        }
      }
      for (const emulator of emulators) {
        for (const suggestion of emulator.emulatorSuggestions) {
          if (suggestion != ext) continue;
          const fuzzyValue = fuzzy(basename, game.mName);
          options.push({
            type: "emulator",
            filename: this.shescape.escape(filename),
            match: fuzzyValue,
            emulatorId: emulator.launchId,

            icon: emulator.gameVersion.game.mIconObjectId,
            gameName: emulator.gameVersion.game.mName,
            versionName: (emulator.gameVersion.displayName ??
              emulator.gameVersion.versionPath)!,
            launchName: emulator.name,
            platform: emulator.platform,
          });
        }
      }
    }

    const sortedOptions = options.sort((a, b) => b.match - a.match);

    return sortedOptions;
  }

  // Checks are done in least to most expensive order
  async checkUnimportedGamePath(libraryId: string, libraryPath: string) {
    const hasGame =
      (await prisma.game.count({
        where: { libraryId, libraryPath },
      })) > 0;
    if (hasGame) return false;

    return true;
  }

  /*
  Game creation happens in metadata, because it's primarily a metadata object

  async createGame(libraryId: string, libraryPath: string, game: Omit<Game, "libraryId" | "libraryPath">) {

  }
  */

  async importVersion(
    gameId: string,
    version: UnimportedVersionInformation,
    metadata: typeof ImportVersion.infer,
    parentTask?: TaskRunContext,
  ) {
    const taskKey = createVersionImportTaskKey(gameId, version.identifier);

    await validateVersionMetadata(metadata.id, metadata);

    const game = await prisma.game.findUnique({
      where: { id: gameId },
      select: { mName: true, libraryId: true, libraryPath: true, type: true },
    });
    if (!game || !game.libraryId) return undefined;

    if (game.type === GameType.Dependency && !metadata.onlySetup)
      throw createError({
        statusCode: 400,
        message: "Dependencies can only be in setup-only mode.",
      });

    const library = this.libraries.get(game.libraryId);
    if (!library) return undefined;

    const unimportedVersion =
      version.type === "depot"
        ? await prisma.unimportedGameVersion.findUnique({
            where: { id: version.identifier },
          })
        : undefined;

    return await taskHandler.create(
      {
        key: taskKey,
        taskGroup: "import:version",
        name: `Importing version ${version.name} for ${game.mName}`,
        acls: ["system:import:version:read"],
        async run({ progress, logger }) {
          let versionPath: string | null = null;
          let manifest;
          let fileList;

          if (version.type === "local") {
            versionPath = version.identifier;
            // First, create the manifest via droplet.
            // This takes up 90% of our progress, so we wrap it in a *0.9

            manifest = await library.generateDropletManifest(
              game.libraryPath,
              versionPath,
              (value) => {
                progress(value * 0.9);
              },
              (value) => {
                logger.info(value);
              },
            );
            fileList = await library.versionReaddir(
              game.libraryPath,
              versionPath,
            );
            logger.info("Created manifest successfully!");
          } else if (version.type === "depot" && unimportedVersion) {
            manifest = castManifest(unimportedVersion.manifest);
            fileList = unimportedVersion.fileList;
            progress(90);
          } else {
            throw "Could not find or create manifest for this version.";
          }

          const largestIndex = await prisma.gameVersion.findFirst({
            where: { gameId: gameId },
            orderBy: {
              versionIndex: "desc",
            },
            select: {
              versionIndex: true,
            },
          });
          const currentIndex = largestIndex ? largestIndex.versionIndex + 1 : 0;

          // Then, create the database object
          const newVersion = await prisma.gameVersion.create({
            data: {
              game: {
                connect: {
                  id: gameId,
                },
              },

              displayName: metadata.displayName ?? null,

              versionPath,
              dropletManifest: manifest,
              fileList,
              versionIndex: currentIndex,
              delta: metadata.delta,
              baseVersion: metadata.delta
                ? { connect: { versionId: metadata.baseVersionId! } }
                : undefined,

              onlySetup: metadata.onlySetup,
              setups: {
                createMany: {
                  data: buildSetupCreateData(metadata.setups),
                },
              },

              launches: {
                createMany: {
                  data: !metadata.onlySetup
                    ? buildLaunchCreateData(metadata.launches, game.type)
                    : [],
                },
              },
            },
          });
          logger.info("Successfully created version!");

          notificationSystem.systemPush({
            nonce: `version-create-${gameId}-${version}`,
            title: `'${game.mName}' ('${version.name}') finished importing.`,
            description: `Drop finished importing version ${version.name} for ${game.mName}.`,
            actions: [`View|/admin/library/${gameId}`],
            acls: ["system:import:version:read"],
          });

          // Ensure cache is filled (also pre-caches the manifest)
          try {
            await gameSizeManager.getVersionSize(newVersion.versionId);
          } catch (e) {
            logger.warn(`Failed to pre-cache game size and manifest: ${e}`);
          }

          if (version.type === "depot") {
            // SAFETY: we can only reach this if the type is depot and identifier is valid
            // eslint-disable-next-line drop/no-prisma-delete
            await prisma.unimportedGameVersion.delete({
              where: {
                id: version.identifier,
              },
            });
          }
          progress(100);
        },
      },
      parentTask,
    );
  }

  /**
   * Re-scans a "local" (filesystem-backed) version's existing versionPath
   * and regenerates its manifest/fileList in place, instead of requiring a
   * delete-and-reimport (which mints a new versionId and breaks any delta
   * chain built on top of the old one).
   *
   * Update-mode versions only make sense against the files they're applied
   * on top of, so this resyncs the version's whole update family with it:
   * its base chain down to the full version, and every update built on that
   * full version.
   */
  async resyncLocalVersion(
    gameId: string,
    versionId: string,
    parentTask?: TaskRunContext,
  ) {
    const existing = await prisma.gameVersion.findFirst({
      where: { versionId, gameId },
      select: { versionPath: true },
    });
    if (!existing) return undefined;
    if (existing.versionPath === null)
      throw createError({
        statusCode: 400,
        message:
          "This version has no on-disk path to resync from (it's a depot-imported version).",
      });
    const versionPath = existing.versionPath;

    const family = await fetchUpdateFamily(gameId, versionId);

    const game = await prisma.game.findUnique({
      where: { id: gameId },
      select: { mName: true, libraryId: true, libraryPath: true },
    });
    if (!game || !game.libraryId) return undefined;

    const library = this.libraries.get(game.libraryId);
    if (!library) return undefined;

    const taskKey = createVersionResyncTaskKey(gameId, versionId);
    const others = family.length - 1;

    return await taskHandler.create(
      {
        key: taskKey,
        taskGroup: "import:version",
        name: `Resyncing version ${versionPath}${others > 0 ? ` and ${others} related version(s)` : ""} for ${game.mName}`,
        acls: ["system:import:version:read"],
        run: async ({ progress, logger }) => {
          await this.rescanLocalVersions(
            gameId,
            library,
            game.libraryPath,
            family,
            { progress, logger },
            0,
            95,
          );

          notificationSystem.systemPush({
            nonce: `version-resync-${gameId}-${versionId}`,
            title: `'${game.mName}' ('${versionPath}') finished resyncing.`,
            description: `Drop finished resyncing version ${versionPath}${others > 0 ? ` and ${others} related version(s)` : ""} for ${game.mName}.`,
            actions: [`View|/admin/library/${gameId}`],
            acls: ["system:import:version:read"],
          });

          await this.invalidateFamilyCaches(gameId, family, logger);
          progress(100);
        },
      },
      parentTask,
    );
  }

  /**
   * Commits a staged depot upload (UnimportedGameVersion) onto an existing
   * "depot" (chunk-uploaded) version's files, replacing its manifest/fileList
   * in place rather than minting a new versionId. The local versions in its
   * update family are resynced from disk along with it, same as
   * resyncLocalVersion.
   */
  async commitDepotReplacement(
    gameId: string,
    versionId: string,
    unimportedVersionId: string,
    parentTask?: TaskRunContext,
  ) {
    const existing = await prisma.gameVersion.findFirst({
      where: { versionId, gameId },
      select: { versionPath: true },
    });
    if (!existing) return undefined;
    if (existing.versionPath !== null)
      throw createError({
        statusCode: 400,
        message:
          "This version has an on-disk path and isn't depot-imported - use resync instead.",
      });

    const unimportedVersion = await prisma.unimportedGameVersion.findFirst({
      where: { id: unimportedVersionId, gameId },
    });
    if (!unimportedVersion)
      throw createError({
        statusCode: 400,
        message: "Could not find a staged upload with that ID for this game.",
      });

    const family = await fetchUpdateFamily(gameId, versionId);

    const game = await prisma.game.findUnique({
      where: { id: gameId },
      select: { mName: true, libraryId: true, libraryPath: true },
    });
    if (!game) return undefined;
    const library = game.libraryId
      ? this.libraries.get(game.libraryId)
      : undefined;

    const taskKey = createVersionResyncTaskKey(gameId, versionId);

    return await taskHandler.create(
      {
        key: taskKey,
        taskGroup: "import:version",
        name: `Replacing files for version ${unimportedVersion.versionName} on ${game.mName}`,
        acls: ["system:import:version:read"],
        run: async ({ progress, logger }) => {
          const manifest = castManifest(unimportedVersion.manifest);
          const fileList = unimportedVersion.fileList;

          const updated = await prisma.gameVersion.updateMany({
            where: { versionId },
            data: { dropletManifest: manifest, fileList, negativeFileList: [] },
          });
          if (updated.count === 0)
            throw `Version ${versionId} disappeared during replacement.`;
          logger.info("Successfully replaced version files!");
          // This version's chunk ids just changed, so the depot's cached
          // manifest for it is stale and would 404 every chunk the new
          // manifest lists.
          await TORRENTIAL_SERVICE.invalidateDownloadContext(gameId, versionId);
          progress(20);

          const related = family.filter((v) => v.versionId !== versionId);
          if (related.some((v) => v.versionPath !== null)) {
            if (!library)
              throw "The game's library is unavailable, so the related local versions couldn't be resynced.";
            await this.rescanLocalVersions(
              gameId,
              library,
              game.libraryPath,
              related,
              { progress, logger },
              20,
              95,
            );
          }

          notificationSystem.systemPush({
            nonce: `version-depot-replace-${gameId}-${versionId}`,
            title: `'${game.mName}' finished replacing files.`,
            description: `Drop finished replacing files for a version of ${game.mName}.`,
            actions: [`View|/admin/library/${gameId}`],
            acls: ["system:import:version:read"],
          });

          await this.invalidateFamilyCaches(gameId, family, logger);

          // eslint-disable-next-line drop/no-prisma-delete
          await prisma.unimportedGameVersion.delete({
            where: { id: unimportedVersionId },
          });

          progress(100);
        },
      },
      parentTask,
    );
  }

  /**
   * Rescans each local version in `versions` from disk, in order, spreading
   * progress over [from, to]. Depot-imported versions have nothing on disk
   * to rescan and their stored files haven't changed, so they're skipped.
   */
  private async rescanLocalVersions(
    gameId: string,
    library: LibraryProvider<unknown>,
    libraryPath: string,
    versions: { versionId: string; versionPath: string | null }[],
    { progress, logger }: Pick<TaskRunContext, "progress" | "logger">,
    from: number,
    to: number,
  ) {
    const local = versions.filter((v) => v.versionPath !== null);
    for (const version of versions) {
      if (version.versionPath === null)
        logger.info(
          `Skipping depot-imported version ${version.versionId}: it has no files on disk to rescan.`,
        );
    }

    const step = (to - from) / Math.max(local.length, 1);
    for (const [i, version] of local.entries()) {
      const versionPath = version.versionPath!;
      const start = from + step * i;
      logger.info(`Rescanning ${versionPath}...`);

      const previous = await prisma.gameVersion.findUniqueOrThrow({
        where: { versionId: version.versionId },
        select: { fileList: true, negativeFileList: true },
      });

      const manifest = await library.generateDropletManifest(
        libraryPath,
        versionPath,
        (value) => {
          progress(start + (value / 100) * step);
        },
        (value) => {
          logger.info(value);
        },
      );
      const fileList = await library.versionReaddir(libraryPath, versionPath);

      // Files that were previously part of this version but are gone from
      // disk now are recorded as removed, same as a delta version tracks
      // files it removes from its base. Earlier removals are kept (unless
      // the file came back), so rescanning an unchanged version is a no-op.
      const present = new Set(fileList);
      const negativeFileList = [
        ...new Set([
          ...previous.negativeFileList,
          ...previous.fileList.filter((f) => !present.has(f)),
        ]),
      ].filter((f) => !present.has(f));

      const updated = await prisma.gameVersion.updateMany({
        where: { versionId: version.versionId },
        data: { dropletManifest: manifest, fileList, negativeFileList },
      });
      if (updated.count === 0)
        throw `Version ${version.versionId} disappeared during resync.`;
      logger.info(`Resynced ${versionPath}.`);

      // This version's chunk ids just changed, so the depot's cached
      // manifest for it is stale and would 404 every chunk the new manifest
      // lists.
      await TORRENTIAL_SERVICE.invalidateDownloadContext(
        gameId,
        version.versionId,
      );
    }
    progress(to);
  }

  /**
   * After any version in an update family changes files, every member's
   * resolved manifest and size may have changed with it.
   */
  private async invalidateFamilyCaches(
    gameId: string,
    family: { versionId: string }[],
    logger: TaskRunContext["logger"],
  ) {
    for (const version of family) {
      await invalidateManifestCache(version.versionId);
      await gameSizeManager.invalidateVersion(version.versionId);
    }
    await gameSizeManager.invalidateGame(gameId);

    for (const version of family) {
      try {
        await gameSizeManager.getVersionSize(version.versionId);
      } catch (e) {
        logger.warn(`Failed to pre-cache game size and manifest: ${e}`);
      }
    }
  }

  async peekFile(
    libraryId: string,
    game: string,
    version: string,
    filename: string,
  ) {
    const library = this.libraries.get(libraryId);
    if (!library) return undefined;
    return await library.peekFile(game, version, filename);
  }

  async deleteGameVersion(gameId: string, version: string) {
    // The FK would just null their base out, leaving update-mode versions
    // with nothing to apply on top of.
    const dependents = await prisma.gameVersion.findMany({
      where: { gameId, baseVersionId: version },
      select: { versionId: true, displayName: true, versionPath: true },
    });
    if (dependents.length > 0)
      throw createError({
        statusCode: 409,
        message: `${dependents.length} update-mode version(s) are applied on top of this version: ${dependents.map((v) => v.displayName ?? v.versionPath ?? v.versionId).join(", ")}. Change their base version or delete them first.`,
      });

    await prisma.gameVersion.deleteMany({
      where: {
        gameId: gameId,
        versionId: version,
      },
    });
  }

  async deleteGame(gameId: string) {
    await prisma.game.deleteMany({
      where: {
        id: gameId,
      },
    });
    // Delete all game versions that depended on this game
    await prisma.gameVersion.deleteMany({
      where: {
        launches: {
          some: {
            emulator: {
              gameVersion: {
                gameId,
              },
            },
          },
        },
      },
    });
  }
}

export const libraryManager = new LibraryManager();
export default libraryManager;
