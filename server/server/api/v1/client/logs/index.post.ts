import { type } from "arktype";
import { ClientCapabilities } from "~/prisma/client/enums";
import { throwingArktype } from "~/server/arktype";
import clientLogManager, {
  clientLogSettings,
  triggerFromSlug,
} from "~/server/internal/clientlogs";
import { defineClientEventHandler } from "~/server/internal/clients/event-handler";

const UploadLogs = type({
  trigger: "'manual' | 'crash' | 'launch'",
  version: "0 < string <= 128",
  arch: "0 < string <= 32",
  session: "string <= 128",
  "gameId?": "string <= 64",
  "summary?": {
    title: "0 < string <= 256",
    "detail?": "string <= 2048",
    "lastError?": "string <= 2048",
  },
  environment: type(["string <= 128", "string <= 4096", "boolean"]).array(),
  files: type({
    name: "0 < string <= 256",
    label: "string <= 128",
    path: "string <= 1024",
    content: "string",
    "truncated?": "boolean",
  })
    .array()
    .atMostLength(32),
}).configure(throwingArktype);

// A bundle is sent as one JSON body: the client caps each file, so the whole
// thing stays well under the configured maximum.
export default defineClientEventHandler(
  async (h3, { fetchClient, fetchUser }) => {
    const client = await fetchClient();
    if (!client.capabilities.includes(ClientCapabilities.ClientLogs))
      throw createError({
        statusCode: 403,
        statusMessage: "Capability not allowed.",
      });

    const settings = await clientLogSettings();
    const maxBytes = settings.maxSizeMB * 1024 * 1024;
    const declared = Number(getHeader(h3, "Content-Length") ?? 0);
    if (declared > maxBytes)
      throw createError({
        statusCode: 413,
        statusMessage: `Log bundle is larger than ${settings.maxSizeMB} MB.`,
      });

    const raw = await readRawBody(h3, "utf8");
    if (!raw)
      throw createError({ statusCode: 400, statusMessage: "Empty body." });
    if (Buffer.byteLength(raw) > maxBytes)
      throw createError({
        statusCode: 413,
        statusMessage: `Log bundle is larger than ${settings.maxSizeMB} MB.`,
      });

    let body;
    try {
      body = UploadLogs(JSON.parse(raw));
    } catch (e) {
      throw createError({
        statusCode: 400,
        statusMessage: e instanceof Error ? e.message : `${e}`,
      });
    }

    // Only what someone explicitly sent gets through when automatic sending
    // is turned off; the client just logs that it wasn't accepted.
    if (body.trigger !== "manual" && !settings.autoSend)
      return { accepted: false };

    const user = await fetchUser();
    const bundle = await clientLogManager.create(
      { id: client.id, name: client.name },
      user.username,
      { ...body, trigger: triggerFromSlug(body.trigger)! },
    );

    return { accepted: true, id: bundle.id };
  },
);
