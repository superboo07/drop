import aclManager from "~/server/internal/acls";
import prisma from "~/server/internal/db/database";
import {
  clientLogSettings,
  serializeBundle,
} from "~/server/internal/clientlogs";

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["client-logs:read"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const bundles = await prisma.clientLogBundle.findMany({
    orderBy: { receivedAt: "desc" },
    take: 500,
  });

  return {
    bundles: bundles.map((bundle) => {
      const { environment: _environment, ...rest } = serializeBundle(bundle);
      return rest;
    }),
    settings: await clientLogSettings(),
  };
});
