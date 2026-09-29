import aclManager from "~/server/internal/acls";
import clientReleaseManager from "~/server/internal/clientreleases";

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["updater:delete"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const id = getRouterParam(h3, "id");
  if (!id)
    throw createError({ statusCode: 400, message: "Missing release ID" });

  if (!(await clientReleaseManager.delete(id)))
    throw createError({ statusCode: 404, message: "Release not found" });

  return { success: true };
});
