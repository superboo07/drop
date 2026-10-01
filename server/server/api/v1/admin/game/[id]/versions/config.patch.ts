import { type } from "arktype";
import { Platform } from "~/prisma/client/enums";
import { readDropValidatedBody, throwingArktype } from "~/server/arktype";
import aclManager from "~/server/internal/acls";
import prisma from "~/server/internal/db/database";
import gameSizeManager from "~/server/internal/gamesize";
import {
  buildLaunchCreateData,
  buildSetupCreateData,
  validateVersionMetadata,
} from "~/server/internal/library";
import {
  fetchDeltaDependents,
  invalidateManifestCache,
} from "~/server/internal/library/manifest";

const UpdateVersionConfig = type({
  versionId: "string",
  displayName: "string?",

  launches: type({
    platform: type.valueOf(Platform),
    name: "string",
    launch: "string",
    umuId: "string?",
    workingDirectory: "string?",
    emulatorId: "string?",
    suggestions: "string[]?",
  }).array(),

  setups: type({
    platform: type.valueOf(Platform),
    launch: "string",
    workingDirectory: "string?",
  }).array(),

  onlySetup: "boolean = false",
  delta: "boolean = false",
  baseVersionId: "string | null?",
}).configure(throwingArktype);

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["game:version:update"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const gameId = getRouterParam(h3, "id")!;
  const body = await readDropValidatedBody(h3, UpdateVersionConfig);

  const existing = await prisma.gameVersion.findFirst({
    where: { versionId: body.versionId, gameId },
    select: { versionId: true, delta: true, baseVersionId: true },
  });
  if (!existing)
    throw createError({ statusCode: 404, statusMessage: "Version not found" });

  const game = await prisma.game.findUnique({
    where: { id: gameId },
    select: { type: true },
  });
  if (!game)
    throw createError({ statusCode: 404, statusMessage: "Game not found" });

  await validateVersionMetadata(gameId, body, body.versionId);
  const baseVersionId = body.delta ? (body.baseVersionId ?? null) : null;

  await prisma.$transaction([
    prisma.launchConfiguration.deleteMany({
      where: { versionId: body.versionId },
    }),
    prisma.setupConfiguration.deleteMany({
      where: { versionId: body.versionId },
    }),
    prisma.gameVersion.updateMany({
      where: { versionId: body.versionId, gameId },
      data: {
        displayName: body.displayName ?? null,
        delta: body.delta,
        baseVersionId: baseVersionId,
        onlySetup: body.onlySetup,
      },
    }),
    prisma.launchConfiguration.createMany({
      data: buildLaunchCreateData(
        body.onlySetup ? [] : body.launches,
        game.type,
      ).map((v) => ({ ...v, versionId: body.versionId })),
    }),
    prisma.setupConfiguration.createMany({
      data: buildSetupCreateData(body.setups).map((v) => ({
        ...v,
        versionId: body.versionId,
      })),
    }),
  ]);

  // delta and its base are the only fields here that affect manifest chain
  // resolution - for this version and everything built on top of it.
  if (
    body.delta !== existing.delta ||
    baseVersionId !== existing.baseVersionId
  ) {
    const dependents = await fetchDeltaDependents(gameId, body.versionId);
    for (const target of [
      body.versionId,
      ...dependents.map((d) => d.versionId),
    ]) {
      await invalidateManifestCache(target);
      await gameSizeManager.invalidateVersion(target);
    }
    await gameSizeManager.invalidateGame(gameId);
  }

  return {};
});
