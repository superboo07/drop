import aclManager from "~/server/internal/acls";
import prisma from "~/server/internal/db/database";
import clientReleaseManager, {
  branchesVisibleTo,
  clientReleaseBranches,
  serializeRelease,
} from "~/server/internal/clientreleases";

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["updater:read"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const id = getRouterParam(h3, "id");
  if (!id)
    throw createError({ statusCode: 400, message: "Missing release ID" });

  const release = await prisma.clientRelease.findUnique({
    where: { id },
    include: { uploader: { select: { username: true, displayName: true } } },
  });
  if (!release)
    throw createError({ statusCode: 404, message: "Release not found" });

  const offered = await clientReleaseManager.offeredReleases();
  // What clients on this release's branch would get instead if it stopped
  // being offered
  const fallback = await prisma.clientRelease.findFirst({
    where: {
      target: release.target,
      arch: release.arch,
      id: { not: release.id },
      branch: { in: branchesVisibleTo(release.branch) },
      publishedAt: { not: null },
      withdrawnAt: null,
    },
    orderBy: { publishedAt: "desc" },
  });

  return {
    ...serializeRelease(release),
    uploader: release.uploader,
    status: clientReleaseManager.status(release, offered),
    offeredOn: clientReleaseManager
      .offeredOn(release, offered)
      .map((branch) => clientReleaseBranches[branch]),
    fallback: fallback ? { id: fallback.id, tag: fallback.tag } : null,
  };
});
