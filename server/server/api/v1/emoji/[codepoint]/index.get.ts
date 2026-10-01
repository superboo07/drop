import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import path from "node:path";
import aclManager from "~/server/internal/acls";

// In the built server this resolves to the copy in .output/server/node_modules,
// which nuxt.config.ts's "compiled" hook fills with the SVGs
const twemojiSvgDir = path.join(
  path.dirname(
    createRequire(import.meta.url).resolve("@discordapp/twemoji/package.json"),
  ),
  "dist",
  "svg",
);

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.hasACL(h3, [
    "system:setup",
    "user:emoji:read",
  ]);
  if (!allowed)
    throw createError({
      statusCode: 403,
      statusMessage: "Requires authentication",
    });

  const codepoint = getRouterParam(h3, "codepoint");
  if (!codepoint) {
    throw createError({
      statusCode: 400,
      statusMessage: "Missing codepoint parameter",
    });
  }

  // Twemoji file names are hex codepoints joined by dashes; anything else
  // (e.g. a path) can't be an emoji
  const asset = /^[0-9a-f]+(-[0-9a-f]+)*$/.test(codepoint)
    ? await readFile(path.join(twemojiSvgDir, `${codepoint}.svg`)).catch(
        () => undefined,
      )
    : undefined;

  if (!asset) {
    throw createError({
      statusCode: 404,
      statusMessage: "Emoji not found",
    });
  }

  // Set proper content type for SVG
  setResponseHeader(h3, "Content-Type", "image/svg+xml");
  setResponseHeader(h3, "Cache-Control", "private, max-age=31536000");

  return asset;
});
