import aclManager from "~/server/internal/acls";
import clientLogManager from "~/server/internal/clientlogs";

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["client-logs:delete"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const id = getRouterParam(h3, "id");
  if (!id) throw createError({ statusCode: 400, message: "Missing bundle ID" });

  if (!(await clientLogManager.delete(id)))
    throw createError({ statusCode: 404, message: "Log bundle not found" });

  return { success: true };
});
