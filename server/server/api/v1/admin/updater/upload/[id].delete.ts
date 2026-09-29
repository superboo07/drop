import aclManager from "~/server/internal/acls";
import clientReleaseManager from "~/server/internal/clientreleases";

// Discards an upload that was never turned into a release (form cancelled)
export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["updater:new"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const id = getRouterParam(h3, "id");
  if (!id) throw createError({ statusCode: 400, message: "Missing upload ID" });

  await clientReleaseManager.discardUpload(id);
  return { success: true };
});
