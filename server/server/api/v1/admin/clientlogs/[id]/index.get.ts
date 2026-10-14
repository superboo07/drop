import aclManager from "~/server/internal/acls";
import prisma from "~/server/internal/db/database";
import { serializeBundle } from "~/server/internal/clientlogs";

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["client-logs:read"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const id = getRouterParam(h3, "id");
  if (!id) throw createError({ statusCode: 400, message: "Missing bundle ID" });

  const bundle = await prisma.clientLogBundle.findUnique({ where: { id } });
  if (!bundle)
    throw createError({ statusCode: 404, message: "Log bundle not found" });

  return serializeBundle(bundle);
});
