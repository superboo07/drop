import { type } from "arktype";
import { readDropValidatedBody, throwingArktype } from "~/server/arktype";
import aclManager from "~/server/internal/acls";
import sessionHandler from "~/server/internal/session";
import clientReleaseManager, {
  branchFromSlug,
  serializeRelease,
} from "~/server/internal/clientreleases";

const CreateRelease = type({
  uploadId: "string.uuid",
  tag: "0 < string <= 64",
  arch: "'x86_64' | 'aarch64'",
  branch: "'release' | 'test'",
  notes: "string = ''",
  required: "boolean = false",
  publish: "boolean = true",
}).configure(throwingArktype);

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["updater:new"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const body = await readDropValidatedBody(h3, CreateRelease);
  const session = await sessionHandler.getSession(h3);
  const token = await aclManager.getAPIToken(h3);

  const release = await clientReleaseManager.create({
    ...body,
    branch: branchFromSlug(body.branch)!,
    tag: body.tag.trim(),
    uploaderId: session?.authenticated?.userId,
    uploaderToken: token?.name,
  });

  return serializeRelease(release);
});
