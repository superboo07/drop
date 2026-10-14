/*
Log bundles sent by desktop clients, browsed in Admin > Clients > Logs.

A bundle is one upload: the client's current and previous log, a failed
game's launch logs, any crash file, and a snapshot of
selected environment variables. Each file is stored as-is under
<data>/client-logs/<bundle id>/<index>.log; the ClientLogBundle row holds
everything the admin pages show without reading them.

Bundles are pruned to the newest `clientLogsPerClient` per client on upload,
and those older than `clientLogsRetentionDays` by the daily cleanup task.
*/
import fs from "node:fs";
import path from "node:path";
import { gzipSync } from "node:zlib";
import prisma from "../db/database";
import { systemConfig } from "../config/sys-conf";
import { applicationSettings } from "../config/application-configuration";
import { logger } from "../logging";
import type { ClientLogTrigger } from "~/prisma/client/enums";
import type { ClientLogBundleModel } from "~/prisma/client/models";
import {
  countLogProblems,
  type ClientLogEnvironment,
  type ClientLogFile,
  type ClientLogSummary,
  type ClientLogTriggerSlug,
} from "~/utils/clientLogs";

export const clientLogTriggers: Record<ClientLogTrigger, ClientLogTriggerSlug> =
  {
    Manual: "manual",
    Crash: "crash",
    LaunchFailed: "launch",
  };

export function triggerFromSlug(slug: string): ClientLogTrigger | undefined {
  return (
    Object.entries(clientLogTriggers) as Array<
      [ClientLogTrigger, ClientLogTriggerSlug]
    >
  ).find(([, value]) => value === slug)?.[0];
}

export type ClientLogUpload = {
  trigger: ClientLogTrigger;
  version: string;
  arch: string;
  session: string;
  gameId?: string;
  summary?: ClientLogSummary;
  environment: ClientLogEnvironment;
  files: Array<{
    name: string;
    label: string;
    path: string;
    content: string;
    truncated?: boolean;
  }>;
};

export async function clientLogSettings() {
  return {
    autoSend: await applicationSettings.get("clientLogsAutoSend"),
    retentionDays: await applicationSettings.get("clientLogsRetentionDays"),
    perClient: await applicationSettings.get("clientLogsPerClient"),
    maxSizeMB: await applicationSettings.get("clientLogsMaxSizeMB"),
  };
}

/** What the admin pages get for a bundle (without file contents) */
export function serializeBundle(bundle: ClientLogBundleModel) {
  return {
    id: bundle.id,
    clientId: bundle.clientId,
    clientName: bundle.clientName,
    userName: bundle.userName,
    trigger: clientLogTriggers[bundle.trigger],
    version: bundle.version,
    arch: bundle.arch,
    session: bundle.session,
    gameId: bundle.gameId,
    gameName: bundle.gameName,
    summary: bundle.summary as ClientLogSummary | null,
    environment: bundle.environment as ClientLogEnvironment,
    files: bundle.files as ClientLogFile[],
    errors: bundle.errors,
    warnings: bundle.warnings,
    size: bundle.size,
    receivedAt: bundle.receivedAt,
  };
}

class ClientLogManager {
  private baseDir = path.join(systemConfig.getDataFolder(), "client-logs");

  constructor() {
    fs.mkdirSync(this.baseDir, { recursive: true });
  }

  private bundleDir(id: string) {
    return path.join(this.baseDir, path.basename(id));
  }

  filePath(id: string, index: number) {
    return path.join(this.bundleDir(id), `${index}.log`);
  }

  async create(
    client: { id: string; name: string },
    userName: string,
    upload: ClientLogUpload,
  ) {
    const files: ClientLogFile[] = [];
    let errors = 0;
    let warnings = 0;
    let size = 0;
    for (const file of upload.files) {
      const problems = countLogProblems(file.content);
      const bytes = Buffer.byteLength(file.content);
      files.push({
        name: file.name,
        label: file.label,
        path: file.path,
        size: bytes,
        truncated: file.truncated ?? false,
        ...problems,
      });
      errors += problems.errors;
      warnings += problems.warnings;
      size += bytes;
    }

    const game = upload.gameId
      ? await prisma.game.findUnique({
          where: { id: upload.gameId },
          select: { mName: true },
        })
      : null;

    const bundle = await prisma.clientLogBundle.create({
      data: {
        clientId: client.id,
        clientName: client.name,
        userName,
        trigger: upload.trigger,
        version: upload.version,
        arch: upload.arch,
        session: upload.session,
        gameId: upload.gameId,
        gameName: game?.mName,
        summary: upload.summary,
        environment: upload.environment,
        files,
        errors,
        warnings,
        size,
      },
    });

    const dir = this.bundleDir(bundle.id);
    try {
      await fs.promises.mkdir(dir, { recursive: true });
      await Promise.all(
        upload.files.map((file, index) =>
          fs.promises.writeFile(this.filePath(bundle.id, index), file.content),
        ),
      );
    } catch (e) {
      await this.delete(bundle.id);
      throw e;
    }

    await this.pruneClient(client.id);
    return bundle;
  }

  /** Keeps only the newest `clientLogsPerClient` bundles of a client */
  private async pruneClient(clientId: string) {
    const keep = await applicationSettings.get("clientLogsPerClient");
    if (keep <= 0) return;
    const stale = await prisma.clientLogBundle.findMany({
      where: { clientId },
      orderBy: { receivedAt: "desc" },
      skip: keep,
      select: { id: true },
    });
    for (const { id } of stale) await this.delete(id);
  }

  async delete(id: string) {
    const { count } = await prisma.clientLogBundle.deleteMany({
      where: { id },
    });
    await fs.promises.rm(this.bundleDir(id), { recursive: true, force: true });
    return count > 0;
  }

  /** Removes bundles past the retention period, and files without a bundle */
  async cleanup() {
    const days = await applicationSettings.get("clientLogsRetentionDays");
    let removed = 0;
    if (days > 0) {
      const cutoff = new Date(Date.now() - days * 24 * 60 * 60 * 1000);
      const expired = await prisma.clientLogBundle.findMany({
        where: { receivedAt: { lt: cutoff } },
        select: { id: true },
      });
      for (const { id } of expired) if (await this.delete(id)) removed++;
    }

    const known = new Set(
      (await prisma.clientLogBundle.findMany({ select: { id: true } })).map(
        (b) => b.id,
      ),
    );
    for (const entry of await fs.promises.readdir(this.baseDir)) {
      if (known.has(entry)) continue;
      await fs.promises.rm(path.join(this.baseDir, entry), {
        recursive: true,
        force: true,
      });
      logger.info(`removed orphaned client log directory ${entry}`);
    }
    return removed;
  }

  async readFile(bundle: ClientLogBundleModel, index: number) {
    const files = bundle.files as ClientLogFile[];
    if (!Number.isInteger(index) || index < 0 || index >= files.length)
      return undefined;
    try {
      return await fs.promises.readFile(this.filePath(bundle.id, index));
    } catch {
      return undefined;
    }
  }

  /** The whole bundle as a .tar.gz, files under a folder named after it */
  async archive(bundle: ClientLogBundleModel) {
    const folder = archiveName(bundle);
    const files = bundle.files as ClientLogFile[];
    const used = new Set<string>();
    const parts: Buffer[] = [];
    const mtime = Math.floor(bundle.receivedAt.getTime() / 1000);

    const add = (name: string, content: Buffer) => {
      parts.push(tarHeader(`${folder}/${name}`, content.length, mtime));
      parts.push(content);
      const padding = (512 - (content.length % 512)) % 512;
      if (padding) parts.push(Buffer.alloc(padding));
    };

    for (const [index, file] of files.entries()) {
      const content = await this.readFile(bundle, index);
      if (!content) continue;
      let name = file.name.replace(/[^\w.-]+/g, "_").slice(0, 60) || "file";
      if (used.has(name)) name = `${index}-${name}`;
      used.add(name);
      add(name, content);
    }
    add(
      "bundle.json",
      Buffer.from(JSON.stringify(serializeBundle(bundle), null, 2)),
    );
    parts.push(Buffer.alloc(1024));

    return {
      name: `${folder}.tar.gz`,
      content: gzipSync(Buffer.concat(parts)),
    };
  }
}

function archiveName(bundle: ClientLogBundleModel) {
  const client = bundle.clientName.replace(/[^\w.-]+/g, "_").slice(0, 32);
  const time = bundle.receivedAt
    .toISOString()
    .slice(0, 19)
    .replace(/[:T]/g, "-");
  return `drop-logs-${client}-${time}`;
}

/** A ustar header for a plain file; `name` must fit in 100 bytes */
function tarHeader(name: string, size: number, mtime: number) {
  const header = Buffer.alloc(512);
  const write = (value: string, offset: number, length: number) =>
    header.write(value, offset, length, "utf8");
  const octal = (value: number, length: number) =>
    value.toString(8).padStart(length - 1, "0") + "\0";

  write(name, 0, 100);
  write(octal(0o644, 8), 100, 8);
  write(octal(0, 8), 108, 8);
  write(octal(0, 8), 116, 8);
  write(octal(size, 12), 124, 12);
  write(octal(mtime, 12), 136, 12);
  write("        ", 148, 8);
  write("0", 156, 1);
  write("ustar\0", 257, 6);
  write("00", 263, 2);

  let checksum = 0;
  for (const byte of header) checksum += byte;
  write(checksum.toString(8).padStart(6, "0") + "\0 ", 148, 8);
  return header;
}

const clientLogManager = new ClientLogManager();
export default clientLogManager;
