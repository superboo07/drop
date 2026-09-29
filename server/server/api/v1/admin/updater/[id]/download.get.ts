import aclManager from "~/server/internal/acls";
import prisma from "~/server/internal/db/database";
import { sendClientRelease } from "~/server/internal/clientreleases/send";

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["updater:read"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const id = getRouterParam(h3, "id");
  if (!id)
    throw createError({ statusCode: 400, message: "Missing release ID" });

  const release = await prisma.clientRelease.findUnique({ where: { id } });
  if (!release)
    throw createError({ statusCode: 404, message: "Release not found" });

  return sendClientRelease(h3, release);
});
