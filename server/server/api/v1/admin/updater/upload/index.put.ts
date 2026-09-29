import sanitize from "sanitize-filename";
import aclManager from "~/server/internal/acls";
import clientReleaseManager, {
  clientReleaseTargets,
  targetFromSlug,
} from "~/server/internal/clientreleases";

/*
Receives a build as the raw request body (not multipart, so it streams to
disk instead of being buffered in memory). nginx lets this one route through
without a body size limit - see build/nginx.conf.

PUT /api/v1/admin/updater/upload?target=linux-appimage&fileName=Drop.AppImage
*/
export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["updater:new"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const query = getQuery(h3);
  const target = targetFromSlug(query.target?.toString() ?? "");
  if (!target)
    throw createError({ statusCode: 400, message: "Unknown release target." });

  const fileName = sanitize(query.fileName?.toString() ?? "");
  const { extension } = clientReleaseTargets[target];
  if (!fileName.toLowerCase().endsWith(extension.toLowerCase()))
    throw createError({
      statusCode: 400,
      message: `Builds for this platform must be ${extension} files.`,
    });

  return await clientReleaseManager.receiveUpload(
    h3.node.req,
    target,
    fileName,
  );
});
