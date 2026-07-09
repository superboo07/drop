import clientHandler from "~/server/internal/clients/handler";
import { useCertificateAuthority } from "~/server/plugins/ca";
import { logger } from "~/server/internal/logging";

export default defineEventHandler(async (h3) => {
  const body = await readBody(h3);
  const clientId = body.clientId;
  const token = body.token;
  if (!clientId || !token)
    throw createError({
      statusCode: 400,
      statusMessage: "Missing token or client ID from body",
    });

  const metadata = await clientHandler.fetchClient(clientId);
  if (!metadata)
    throw createError({
      statusCode: 403,
      statusMessage: "Invalid client ID",
    });
  if (!metadata.authToken || !metadata.userId)
    throw createError({
      statusCode: 400,
      statusMessage: "Un-authorized client ID",
    });
  if (metadata.authToken !== token)
    throw createError({
      statusCode: 403,
      statusMessage: "Invalid token",
    });

  const certificateAuthority = useCertificateAuthority();

  let bundle;
  try {
    bundle = await certificateAuthority.generateClientCertificate(
      clientId,
      metadata.data.name,
    );
  } catch (e) {
    logger.error(`failed to generate client certificate for ${clientId}: ${e}`);
    throw createError({
      statusCode: 503,
      statusMessage:
        "Failed to generate a client certificate. The server's certificate/depot service may be unavailable - please try again shortly.",
    });
  }

  // finialiseClient consumes the temporary client entry, so a retried or
  // duplicate handshake for the same token lands here with it already gone -
  // that's an expected, recoverable condition, not a server crash.
  let client;
  try {
    client = await clientHandler.finialiseClient(clientId);
  } catch (e) {
    logger.warn(`failed to finalise client ${clientId}: ${e}`);
    throw createError({
      statusCode: 409,
      statusMessage:
        "This sign-in attempt has already been completed or has expired. Please restart the sign-in process.",
    });
  }

  try {
    await certificateAuthority.storeClientCertificate(clientId, bundle);
  } catch (e) {
    logger.error(`failed to store client certificate for ${clientId}: ${e}`);
    throw createError({
      statusCode: 503,
      statusMessage:
        "Failed to store the client certificate. Please try again shortly.",
    });
  }

  return {
    private: bundle.priv,
    certificate: bundle.cert,
    id: client.id,
  };
});
