import { type } from "arktype";
import { readDropValidatedBody, throwingArktype } from "~/server/arktype";
import aclManager from "~/server/internal/acls";
import prisma from "~/server/internal/db/database";

const UpdateVersionOrder = type({
  versions: "string[]",
}).configure(throwingArktype);

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["game:version:update"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const body = await readDropValidatedBody(h3, UpdateVersionOrder);
  const gameId = getRouterParam(h3, "id")!;
  // We expect an array of the version names for this game
  const unsortedVersions = await prisma.gameVersion.findMany({
    where: {
      versionId: { in: body.versions },
    },
    select: {
      versionId: true,
      versionIndex: true,
    },
  });

  const versions = body.versions
    .map((e) => unsortedVersions.find((v) => v.versionId === e))
    .filter((e) => e !== undefined);

  if (versions.length !== unsortedVersions.length)
    throw createError({
      statusCode: 500,
      statusMessage: "Sorting versions yielded less results, somehow.",
    });

  // Order is display-only: update-mode versions name their base explicitly
  // rather than building on whatever sits below them.
  await prisma.$transaction(
    versions.map((version, versionIndex) =>
      prisma.gameVersion.updateMany({
        where: {
          gameId: gameId,
          versionId: version.versionId,
        },
        data: {
          versionIndex: versionIndex,
        },
      }),
    ),
  );

  return versions.map((v) => v.versionId);
});
