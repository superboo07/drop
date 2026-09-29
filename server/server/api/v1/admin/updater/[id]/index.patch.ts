import { type } from "arktype";
import { readDropValidatedBody, throwingArktype } from "~/server/arktype";
import aclManager from "~/server/internal/acls";
import prisma from "~/server/internal/db/database";
import { branchFromSlug } from "~/server/internal/clientreleases";

const UpdateRelease = type({
  "tag?": "0 < string <= 64",
  "notes?": "string",
  "required?": "boolean",
  "branch?": "'release' | 'test'",
  // Drafts can be published; a published release can't go back to a draft
  "publish?": "true",
  "withdrawn?": "boolean",
}).configure(throwingArktype);

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["updater:update"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const id = getRouterParam(h3, "id");
  if (!id)
    throw createError({ statusCode: 400, message: "Missing release ID" });

  const body = await readDropValidatedBody(h3, UpdateRelease);

  const release = await prisma.clientRelease.findUnique({ where: { id } });
  if (!release)
    throw createError({ statusCode: 404, message: "Release not found" });

  const { count } = await prisma.clientRelease.updateMany({
    where: { id },
    data: {
      tag: body.tag?.trim(),
      notes: body.notes,
      required: body.required,
      branch: body.branch ? branchFromSlug(body.branch) : undefined,
      ...(body.publish && !release.publishedAt && { publishedAt: new Date() }),
      ...(body.withdrawn !== undefined && {
        withdrawnAt: body.withdrawn
          ? (release.withdrawnAt ?? new Date())
          : null,
      }),
    },
  });
  if (count === 0)
    throw createError({ statusCode: 404, message: "Release not found" });

  return { success: true };
});
