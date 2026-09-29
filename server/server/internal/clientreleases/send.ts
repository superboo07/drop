import fs from "node:fs";
import type { H3Event } from "h3";
import type { ClientReleaseModel } from "~/prisma/client/models";
import clientReleaseManager from ".";

/** Streams a release's file as a download */
export async function sendClientRelease(
  h3: H3Event,
  release: ClientReleaseModel,
) {
  const filePath = clientReleaseManager.releasePath(release.id);
  try {
    await fs.promises.access(filePath, fs.constants.R_OK);
  } catch {
    throw createError({
      statusCode: 404,
      message: "This release's file is missing from the server.",
    });
  }

  setHeader(h3, "Content-Type", "application/octet-stream");
  setHeader(h3, "Content-Length", Number(release.size));
  setHeader(
    h3,
    "Content-Disposition",
    `attachment; filename="${release.fileName.replaceAll('"', "")}"`,
  );
  setHeader(h3, "X-Drop-Release-SHA256", release.sha256);
  return sendStream(h3, fs.createReadStream(filePath));
}
