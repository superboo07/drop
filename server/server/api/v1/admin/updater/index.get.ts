import aclManager from "~/server/internal/acls";
import prisma from "~/server/internal/db/database";
import clientReleaseManager, {
  clientReleaseArchs,
  clientReleaseBranches,
  clientReleaseTargets,
  plannedClientReleaseTargets,
  serializeRelease,
} from "~/server/internal/clientreleases";
import type {
  ClientReleaseBranch,
  ClientReleaseTarget,
} from "~/prisma/client/enums";

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["updater:read"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const releases = await prisma.clientRelease.findMany({
    orderBy: { uploadedAt: "desc" },
  });
  const offered = await clientReleaseManager.offeredReleases();

  const targets = (
    Object.keys(clientReleaseTargets) as ClientReleaseTarget[]
  ).flatMap((target) =>
    clientReleaseArchs.flatMap((arch) =>
      (Object.keys(clientReleaseBranches) as ClientReleaseBranch[]).map(
        (branch) => {
          const release = offered.get(`${target}/${arch}/${branch}`);
          return {
            target,
            arch,
            branch: clientReleaseBranches[branch],
            label: clientReleaseTargets[target].label,
            offered: release ? serializeRelease(release) : null,
          };
        },
      ),
    ),
  );

  return {
    targets,
    plannedTargets: plannedClientReleaseTargets,
    releases: releases.map((release) => ({
      ...serializeRelease(release),
      status: clientReleaseManager.status(release, offered),
      offeredOn: clientReleaseManager
        .offeredOn(release, offered)
        .map((branch) => clientReleaseBranches[branch]),
    })),
  };
});
