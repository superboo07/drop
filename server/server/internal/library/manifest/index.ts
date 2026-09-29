import cacheHandler from "../../cache";
import prisma from "../../db/database";
import { castManifest, type DropletManifest } from "./utils";

export type DownloadManifestDetails = {
  /***
   * Version ID to manifest
   */
  manifests: { [key: string]: DropletManifest };
  /***
   * File name to version ID
   */
  fileList: { [key: string]: string };
  /***
   * File name to whole-file SHA-256 (hex), for files whose owning version
   * has one recorded. Missing entries mean no server-side hash is available
   * for that file (e.g. it predates hashing support).
   */
  fileHashes: { [key: string]: string };
  /// Size on disk after download
  installSize: number;
  /// Size of download
  downloadSize: number;
};

function convertMap<T>(map: Map<string, T>): { [key: string]: T } {
  return Object.fromEntries(map.entries().toArray());
}
const manifestCache =
  cacheHandler.createCache<DownloadManifestDetails>("manifestCache");

/**
 *
 * @param gameId Game ID
 * @param versionId Version ID
 */
export async function createDownloadManifestDetails(
  versionId: string,
  previous?: string,
  refresh = false,
): Promise<DownloadManifestDetails> {
  const manifestKey = `${versionId}${previous ? `-from-${previous}` : ""}`;
  if ((await manifestCache.has(manifestKey)) && !refresh)
    return (await manifestCache.get(manifestKey))!;
  const mainVersion = await prisma.gameVersion.findUnique({
    where: { versionId },
    select: chainVersionSelect,
  });
  if (!mainVersion)
    throw createError({ statusCode: 404, message: "Version not found" });

  const collectedVersions = mainVersion.delta
    ? await resolveBaseChain(mainVersion.baseVersionId, mainVersion.versionId)
    : [];
  // Apply fileList in lowest priority to newest priority
  const versionOrder = [...collectedVersions, mainVersion];

  const fileList = new Map<string, string>();
  for (const version of versionOrder) {
    for (const file of version.fileList) {
      fileList.set(file, version.versionId);
    }
    for (const negFile of version.negativeFileList) {
      fileList.delete(negFile);
    }
  }

  let installSize = 0;
  let downloadSize = 0;

  const existingChunks = previous
    ? await createDownloadManifestDetails(previous)
    : undefined;

  // Now that we have our file list, filter the manifests
  const manifests = new Map<string, DropletManifest>();
  const fileHashes = new Map<string, string>();
  for (const version of versionOrder) {
    const files = fileList
      .entries()
      .filter(([, versionId]) => version.versionId === versionId)
      .toArray();
    if (files.length == 0) continue;
    const fileNames = Object.fromEntries(files);
    const manifest = castManifest(version.dropletManifest);
    for (const filename of Object.keys(fileNames)) {
      const hash = manifest.fileHashes?.[filename];
      if (hash) fileHashes.set(filename, hash);
    }
    const filteredChunks = Object.fromEntries(
      Object.entries(manifest.chunks).filter(([_, chunkData]) => {
        //if(existingChunks && existingChunks.manifests[version.versionId]?.chunks?.[chunkId]) return false;
        let flag = false;
        chunkData.files.forEach((fileEntry) => {
          if (
            existingChunks &&
            existingChunks.fileList[fileEntry.filename] == version.versionId
          )
            return;
          if (fileNames[fileEntry.filename]) {
            flag = true;
            installSize += fileEntry.length;
          }
        });
        // If we have to download this chunk, add it's length
        if (flag) {
          downloadSize += chunkData.files
            .map((v) => v.length)
            .reduce((a, b) => a + b, 0);
        }
        return flag;
      }),
    );
    manifests.set(version.versionId, {
      ...manifest,
      chunks: filteredChunks,
    });
  }

  const result = {
    fileList: convertMap(fileList),
    fileHashes: convertMap(fileHashes),
    manifests: convertMap(manifests),
    installSize,
    downloadSize,
  };
  await manifestCache.set(manifestKey, result);

  return result;
}

const chainVersionSelect = {
  versionId: true,
  delta: true,
  baseVersionId: true,
  fileList: true,
  negativeFileList: true,
  dropletManifest: true,
} as const;

/**
 * Walks a delta version's base links down to the first full version and
 * returns that chain lowest-priority first (full version, then each delta on
 * top of it), not including the version the walk started from.
 * `startingFrom` is only used to name the version in errors and to catch a
 * chain that loops back on itself.
 */
export async function resolveBaseChain(
  baseVersionId: string | null,
  startingFrom: string,
) {
  const chain = [];
  const seen = new Set([startingFrom]);
  let nextId = baseVersionId;
  while (true) {
    if (!nextId)
      throw createError({
        statusCode: 500,
        message: `Update-mode version ${startingFrom} has no base version to apply on top of.`,
      });
    if (seen.has(nextId))
      throw createError({
        statusCode: 500,
        message: `Update-mode version ${startingFrom} has a base chain that loops back on itself.`,
      });
    seen.add(nextId);

    const next = await prisma.gameVersion.findUnique({
      where: { versionId: nextId },
      select: chainVersionSelect,
    });
    if (!next)
      throw createError({
        statusCode: 500,
        message: `Base version ${nextId} of ${startingFrom} no longer exists.`,
      });
    chain.push(next);
    if (!next.delta) break;
    nextId = next.baseVersionId;
  }
  return chain.reverse();
}

/**
 * Finds every version whose cached manifest resolution depends on
 * `versionId`'s files - every delta version that has it somewhere in its
 * base chain, i.e. all of its descendants in the base-version tree.
 */
export async function fetchDeltaDependents(gameId: string, versionId: string) {
  const versions = await prisma.gameVersion.findMany({
    where: { gameId },
    orderBy: { versionIndex: "asc" },
    select: {
      versionId: true,
      versionIndex: true,
      delta: true,
      baseVersionId: true,
      displayName: true,
      versionPath: true,
    },
  });

  const reached = new Set([versionId]);
  const dependents = [];
  let grew = true;
  while (grew) {
    grew = false;
    for (const version of versions) {
      if (reached.has(version.versionId)) continue;
      if (!version.delta || !version.baseVersionId) continue;
      if (!reached.has(version.baseVersionId)) continue;
      reached.add(version.versionId);
      dependents.push(version);
      grew = true;
    }
  }
  return dependents.sort((a, b) => a.versionIndex - b.versionIndex);
}

/**
 * Every version whose installed files are tied to `versionId`'s through
 * update-mode bases: the full version at the root of its chain, then every
 * update built on that root (side branches included), bases before the
 * updates on top of them. A version that isn't part of any chain comes back
 * on its own.
 */
export async function fetchUpdateFamily(gameId: string, versionId: string) {
  const versions = await prisma.gameVersion.findMany({
    where: { gameId },
    orderBy: { versionIndex: "asc" },
    select: {
      versionId: true,
      versionIndex: true,
      delta: true,
      baseVersionId: true,
      displayName: true,
      versionPath: true,
    },
  });
  const byId = new Map(versions.map((v) => [v.versionId, v]));

  let root = byId.get(versionId);
  if (!root) return [];
  const seen = new Set([root.versionId]);
  while (root.delta && root.baseVersionId) {
    const next = byId.get(root.baseVersionId);
    if (!next || seen.has(next.versionId)) break;
    seen.add(next.versionId);
    root = next;
  }

  const family = [root];
  const reached = new Set([root.versionId]);
  for (let i = 0; i < family.length; i++) {
    for (const version of versions) {
      if (reached.has(version.versionId)) continue;
      if (!version.delta || version.baseVersionId !== family[i].versionId)
        continue;
      reached.add(version.versionId);
      family.push(version);
    }
  }
  return family;
}

/**
 * Busts the cached manifest resolution for a version - needed any time its
 * files or delta-chain topology change, since createDownloadManifestDetails
 * otherwise keeps serving the stale result indefinitely (there is currently
 * no other invalidation path anywhere in the codebase).
 */
export async function invalidateManifestCache(versionId: string) {
  const keys = await manifestCache.getKeys();
  await Promise.all(
    keys
      .filter(
        (k) =>
          k === versionId ||
          k.startsWith(`${versionId}-from-`) ||
          k.endsWith(`-from-${versionId}`),
      )
      .map((k) => manifestCache.remove(k)),
  );
}
