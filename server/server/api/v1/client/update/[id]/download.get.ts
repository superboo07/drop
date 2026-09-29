import { defineClientEventHandler } from "~/server/internal/clients/event-handler";
import prisma from "~/server/internal/db/database";
import { sendClientRelease } from "~/server/internal/clientreleases/send";

export default defineClientEventHandler(async (h3) => {
  const id = getRouterParam(h3, "id");
  if (!id)
    throw createError({ statusCode: 400, message: "Missing release ID" });

  // Clients only ever get builds that are live
  const release = await prisma.clientRelease.findFirst({
    where: { id, publishedAt: { not: null }, withdrawnAt: null },
  });
  if (!release)
    throw createError({ statusCode: 404, message: "Release not found" });

  return sendClientRelease(h3, release);
});
