import clientLogManager from "~/server/internal/clientlogs";
import { defineDropTask } from "..";

export default defineDropTask({
  buildId: () => `cleanup:client-logs:${new Date().toISOString()}`,
  name: "Cleanup Client Logs",
  acls: ["system:maintenance:read"],
  taskGroup: "cleanup:client-logs",
  async run({ progress, logger }) {
    logger.info("Removing expired client logs");
    const removed = await clientLogManager.cleanup();
    logger.info(`Removed ${removed} log bundle(s)`);
    progress(100);
  },
});
