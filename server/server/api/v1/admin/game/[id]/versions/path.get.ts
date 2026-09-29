import { ArkErrors, type } from "arktype";
import aclManager from "~/server/internal/acls";
import prisma from "~/server/internal/db/database";
import gameSizeManager from "~/server/internal/gamesize";
import libraryManager from "~/server/internal/library";
import {
  createDownloadManifestDetails,
  resolveBaseChain,
} from "~/server/internal/library/manifest";

/**
 * Previews the install path an update-mode version would take if it were
 * applied on top of `base`: the chain from the full version up to `base`,
 * plus how the target's files overlap what that chain already installs.
 * The target is either an existing version (`versionId`) or one that hasn't
 * been imported yet (`type` + `identifier`); without one only the chain is
 * returned.
 */
const Query = type({
  base: "string",
  versionId: "string?",
  type: "'depot' | 'local'?",
  identifier: "string?",
});

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["game:read"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const gameId = getRouterParam(h3, "id")!;
  const query = Query(getQuery(h3));
  if (query instanceof ArkErrors)
    throw createError({ statusCode: 400, message: query.summary });

  const base = await prisma.gameVersion.findFirst({
    where: { versionId: query.base, gameId },
    select: {
      versionId: true,
      delta: true,
      baseVersionId: true,
      fileList: true,
      negativeFileList: true,
    },
  });
  if (!base)
    throw createError({ statusCode: 404, message: "Base version not found" });

  const below = base.delta
    ? await resolveBaseChain(base.baseVersionId, base.versionId)
    : [];
  const chain = [...below, base];
  const names = await prisma.gameVersion.findMany({
    where: { versionId: { in: chain.map((v) => v.versionId) } },
    select: { versionId: true, displayName: true, versionPath: true },
  });
  const nameOf = (id: string) => {
    const v = names.find((n) => n.versionId === id);
    return v?.displayName ?? v?.versionPath ?? id;
  };

  const steps = chain.map((v) => ({
    versionId: v.versionId,
    name: nameOf(v.versionId),
    delta: v.delta,
    fileCount: v.fileList.length,
    removedCount: v.negativeFileList.length,
  }));

  let targetFiles: string[] | undefined;
  if (query.versionId) {
    const target = await prisma.gameVersion.findFirst({
      where: { versionId: query.versionId, gameId },
      select: { fileList: true },
    });
    targetFiles = target?.fileList;
  } else if (query.type && query.identifier) {
    targetFiles = await libraryManager.fetchUnimportedVersionFiles(gameId, {
      type: query.type,
      identifier: query.identifier,
    });
  }

  // What a player on `base` has installed, after its whole chain is applied.
  const installed = (await createDownloadManifestDetails(base.versionId))
    .fileList;

  let target = null;
  if (targetFiles) {
    const overwritten = targetFiles.filter((f) => f in installed).length;
    target = {
      fileCount: targetFiles.length,
      overwritten,
      added: targetFiles.length - overwritten,
    };
  }

  const baseSize = await gameSizeManager.getVersionSize(base.versionId);

  return {
    steps,
    baseInstallSize: baseSize?.installSize ?? null,
    baseFileCount: Object.keys(installed).length,
    target,
  };
});
