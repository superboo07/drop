import { type } from "arktype";
import { readDropValidatedBody, throwingArktype } from "~/server/arktype";
import aclManager from "~/server/internal/acls";
import libraryManager from "~/server/internal/library";

const DepotReplaceVersion = type({
  versionId: "string",
  unimportedVersionId: "string",
  force: "boolean = false",
}).configure(throwingArktype);

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["game:version:resync"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const gameId = getRouterParam(h3, "id")!;
  const body = await readDropValidatedBody(h3, DepotReplaceVersion);

  const taskId = await libraryManager.commitDepotReplacement(
    gameId,
    body.versionId,
    body.unimportedVersionId,
    body.force,
  );
  if (!taskId)
    throw createError({
      statusCode: 400,
      statusMessage: "Invalid version or staged upload to replace with",
    });

  return { taskId };
});
