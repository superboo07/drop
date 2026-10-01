import { type } from "arktype";
import { readDropValidatedBody, throwingArktype } from "~/server/arktype";
import aclManager from "~/server/internal/acls";
import prisma from "~/server/internal/db/database";

// Every setting is optional: missing means no preference, which leaves it to
// the client's own default (or whatever the player has chosen). Nulls count
// as missing too, since readDropValidatedBody strips them before validating.
const UpdateProtonDefaults = type({
  versionId: "string",
  protonName: "string?",
  dxvk: "boolean?",
  esync: "boolean?",
  fsync: "boolean?",
  locale: "string?",
  extraEnvVars: "string?",
  winetricks: type("string")
    .array()
    .default(() => []),
}).configure(throwingArktype);

// What winetricks accepts as a verb, including settings like vd=1280x720
const WINETRICKS_VERB = /^[a-zA-Z0-9_.=-]+$/;

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["game:version:update"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const gameId = getRouterParam(h3, "id")!;
  const body = await readDropValidatedBody(h3, UpdateProtonDefaults);

  const version = await prisma.gameVersion.findFirst({
    where: { versionId: body.versionId, gameId },
    select: { versionId: true },
  });
  if (!version)
    throw createError({ statusCode: 404, statusMessage: "Version not found" });

  const winetricks = [
    ...new Set(body.winetricks.map((v) => v.trim()).filter(Boolean)),
  ];
  const badVerb = winetricks.find((v) => !WINETRICKS_VERB.test(v));
  if (badVerb)
    throw createError({
      statusCode: 400,
      statusMessage: `"${badVerb}" isn't a valid winetricks verb.`,
    });

  const locale = body.locale?.trim() || null;
  if (locale && /\s/.test(locale))
    throw createError({
      statusCode: 400,
      statusMessage: `"${locale}" isn't a valid locale.`,
    });

  const data = {
    protonName: body.protonName?.trim() || null,
    dxvk: body.dxvk ?? null,
    esync: body.esync ?? null,
    fsync: body.fsync ?? null,
    locale,
    extraEnvVars: body.extraEnvVars?.trim() || null,
    winetricks,
  };

  const empty =
    winetricks.length === 0 &&
    Object.values(data).every((v) => v === null || v === winetricks);

  // Nothing recommended any more: drop the row, so clients see no defaults
  // rather than an all-null set.
  if (empty) {
    await prisma.protonDefaults.deleteMany({
      where: { versionId: body.versionId },
    });
    return { protonDefaults: null };
  }

  const protonDefaults = await prisma.protonDefaults.upsert({
    where: { versionId: body.versionId },
    create: { versionId: body.versionId, ...data },
    update: data,
  });

  return { protonDefaults };
});
