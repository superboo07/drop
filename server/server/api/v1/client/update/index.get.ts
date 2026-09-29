import { defineClientEventHandler } from "~/server/internal/clients/event-handler";
import clientReleaseManager, {
  branchFromSlug,
  isClientReleaseArch,
  targetFromSlug,
} from "~/server/internal/clientreleases";

/*
Asks whether this client should update.

GET /api/v1/client/update?target=linux-appimage&arch=x86_64&branch=release&current=<release id>&sha256=<hash>

`current` is the release the client last installed, if it knows it; `sha256`
is the hash of the build it's running, used when `current` is missing or no
longer exists. Returns `{ update: null }` when the client is up to date.
*/
export default defineClientEventHandler(async (h3) => {
  const query = getQuery(h3);

  const target = targetFromSlug(query.target?.toString() ?? "");
  if (!target)
    throw createError({ statusCode: 400, message: "Unknown release target." });

  const arch = query.arch?.toString() ?? "";
  if (!isClientReleaseArch(arch))
    throw createError({ statusCode: 400, message: "Unknown architecture." });

  // Older clients don't send a branch; they get release builds
  const branch = branchFromSlug(query.branch?.toString() ?? "release");
  if (!branch)
    throw createError({ statusCode: 400, message: "Unknown branch." });

  return await clientReleaseManager.checkForUpdate({
    target,
    arch,
    branch,
    currentId: query.current?.toString() || undefined,
    currentSha256: query.sha256?.toString() || undefined,
  });
});
