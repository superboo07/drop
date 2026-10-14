import { type } from "arktype";
import { readDropValidatedBody, throwingArktype } from "~/server/arktype";
import aclManager from "~/server/internal/acls";
import { applicationSettings } from "~/server/internal/config/application-configuration";
import { clientLogSettings } from "~/server/internal/clientlogs";

const UpdateSettings = type({
  "autoSend?": "boolean",
  "retentionDays?": "0 <= number.integer <= 3650",
  "perClient?": "0 <= number.integer <= 1000",
  "maxSizeMB?": "1 <= number.integer <= 500",
}).configure(throwingArktype);

export default defineEventHandler(async (h3) => {
  const allowed = await aclManager.allowSystemACL(h3, ["client-logs:update"]);
  if (!allowed) throw createError({ statusCode: 403 });

  const body = await readDropValidatedBody(h3, UpdateSettings);

  if (body.autoSend !== undefined)
    await applicationSettings.set("clientLogsAutoSend", body.autoSend);
  if (body.retentionDays !== undefined)
    await applicationSettings.set(
      "clientLogsRetentionDays",
      body.retentionDays,
    );
  if (body.perClient !== undefined)
    await applicationSettings.set("clientLogsPerClient", body.perClient);
  if (body.maxSizeMB !== undefined)
    await applicationSettings.set("clientLogsMaxSizeMB", body.maxSizeMB);

  return clientLogSettings();
});
