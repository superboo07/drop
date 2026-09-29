/*
Desktop client releases uploaded through Admin > Settings > Updater.

A release's identity is its UUID; the tag ("0.4.2") is only what people see
and may repeat, so a withdrawn or deleted build can be re-uploaded under the
same tag and clients running the old build are still offered the new one.

For each (target, arch, branch) the offered release is the most recently
published one that hasn't been withdrawn. The release branch only considers
release builds; the test branch considers builds from either branch. A client
is offered it whenever the release it runs isn't that one - including when the
offered one is older (a rollback after the newest build was withdrawn, or a
switch from the test branch back to release).
*/
import fs from "node:fs";
import path from "node:path";
import { createHash, randomUUID } from "node:crypto";
import { Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import type { Readable } from "node:stream";
import prisma from "../db/database";
import { systemConfig } from "../config/sys-conf";
import { logger } from "../logging";
import type {
  ClientReleaseArch,
  ClientReleaseBranch,
  ClientReleaseTarget,
} from "~/prisma/client/enums";
import type { ClientReleaseModel } from "~/prisma/client/models";

export const clientReleaseArchs: ClientReleaseArch[] = ["x86_64", "aarch64"];

export const clientReleaseBranches: Record<
  ClientReleaseBranch,
  "release" | "test"
> = {
  Release: "release",
  Test: "test",
};

export function branchFromSlug(slug: string): ClientReleaseBranch | undefined {
  return (
    Object.entries(clientReleaseBranches) as Array<
      [ClientReleaseBranch, string]
    >
  ).find(([, value]) => value === slug)?.[0];
}

/** Which branches' builds a client on `branch` may be offered */
export function branchesVisibleTo(
  branch: ClientReleaseBranch,
): ClientReleaseBranch[] {
  return branch === "Test" ? ["Release", "Test"] : ["Release"];
}

/**
 * build_appimage.sh stamps ".dirty" onto the version of builds made from a
 * tree with uncommitted changes; those go to the test branch.
 */
export function suggestBranch(name: string): ClientReleaseBranch {
  return /[.-]dirty\b/i.test(name) ? "Test" : "Release";
}

type TargetInfo = {
  /** Stable id used in URLs and by the desktop client */
  slug: string;
  label: string;
  /** File extension builds for this target must have */
  extension: string;
};

export const clientReleaseTargets: Record<ClientReleaseTarget, TargetInfo> = {
  LinuxAppImage: {
    slug: "linux-appimage",
    label: "Linux · AppImage",
    extension: ".AppImage",
  },
};

/** Shown on the Updater page as placeholders so the layout has room for them */
export const plannedClientReleaseTargets = [{ label: "Windows · Installer" }];

export function targetFromSlug(slug: string): ClientReleaseTarget | undefined {
  return (
    Object.entries(clientReleaseTargets) as Array<
      [ClientReleaseTarget, TargetInfo]
    >
  ).find(([, info]) => info.slug === slug)?.[0];
}

export function isClientReleaseArch(arch: string): arch is ClientReleaseArch {
  return (clientReleaseArchs as string[]).includes(arch);
}

export type ClientReleaseStatus =
  | "draft"
  | "offered"
  | "superseded"
  | "withdrawn";

const UPLOAD_MAX_AGE_MS = 24 * 60 * 60 * 1000;

class ClientReleaseManager {
  private baseDir = path.join(systemConfig.getDataFolder(), "client-releases");
  private uploadDir = path.join(this.baseDir, "uploads");

  constructor() {
    fs.mkdirSync(this.uploadDir, { recursive: true });
  }

  releasePath(id: string) {
    return path.join(this.baseDir, path.basename(id));
  }

  private uploadPath(uploadId: string) {
    return path.join(this.uploadDir, path.basename(uploadId));
  }

  /**
   * Streams an uploaded build to a temporary file, hashing it on the way.
   * The upload is turned into a release by `create`, so the admin can fill
   * in the form while the file is still uploading.
   */
  async receiveUpload(
    source: Readable,
    target: ClientReleaseTarget,
    fileName: string,
  ) {
    await this.sweepUploads();

    const uploadId = randomUUID();
    const filePath = this.uploadPath(uploadId);
    const hash = createHash("sha256");
    let size = 0;

    try {
      await pipeline(
        source,
        new Transform({
          transform(chunk: Buffer, _encoding, callback) {
            hash.update(chunk);
            size += chunk.length;
            callback(null, chunk);
          },
        }),
        fs.createWriteStream(filePath),
      );
    } catch (e) {
      await fs.promises.rm(filePath, { force: true });
      throw e;
    }

    const inspected = await this.inspectBuild(target, filePath);
    if (typeof inspected === "string") {
      await fs.promises.rm(filePath, { force: true });
      throw createError({ statusCode: 400, message: inspected });
    }

    const upload: PendingUpload = {
      uploadId,
      target,
      fileName,
      size,
      sha256: hash.digest("hex"),
      arch: inspected.arch,
    };
    await fs.promises.writeFile(`${filePath}.json`, JSON.stringify(upload));

    return {
      ...upload,
      suggestedTag: suggestTag(fileName),
      suggestedBranch: clientReleaseBranches[suggestBranch(fileName)],
    };
  }

  async discardUpload(uploadId: string) {
    const filePath = this.uploadPath(uploadId);
    await fs.promises.rm(filePath, { force: true });
    await fs.promises.rm(`${filePath}.json`, { force: true });
  }

  /** Drops uploads that were never turned into a release */
  private async sweepUploads() {
    const cutoff = Date.now() - UPLOAD_MAX_AGE_MS;
    for (const entry of await fs.promises.readdir(this.uploadDir)) {
      const entryPath = path.join(this.uploadDir, entry);
      try {
        const stat = await fs.promises.stat(entryPath);
        if (stat.mtimeMs < cutoff) await fs.promises.rm(entryPath);
      } catch (e) {
        logger.warn(`failed to clean up client release upload ${entry}: ${e}`);
      }
    }
  }

  /**
   * Checks the file really is a build for the target, and reads its
   * architecture from the ELF header. Returns an error message otherwise.
   */
  private async inspectBuild(
    target: ClientReleaseTarget,
    filePath: string,
  ): Promise<{ arch: ClientReleaseArch } | string> {
    switch (target) {
      case "LinuxAppImage": {
        const header = Buffer.alloc(20);
        const handle = await fs.promises.open(filePath, "r");
        try {
          await handle.read(header, 0, header.length, 0);
        } finally {
          await handle.close();
        }
        // ELF magic, then the AppImage type 2 magic "AI\x02" in e_ident padding
        if (
          header.readUInt32BE(0) !== 0x7f454c46 ||
          header.toString("latin1", 8, 10) !== "AI" ||
          header[10] !== 0x02
        )
          return "This file isn't a type 2 AppImage.";

        const machine = header.readUInt16LE(18);
        if (machine === 0x3e) return { arch: "x86_64" };
        if (machine === 0xb7) return { arch: "aarch64" };
        return `This AppImage is built for an unsupported CPU architecture (ELF machine ${machine}).`;
      }
    }
  }

  async create(options: {
    uploadId: string;
    tag: string;
    arch: ClientReleaseArch;
    branch: ClientReleaseBranch;
    notes: string;
    required: boolean;
    publish: boolean;
    uploaderId?: string;
    uploaderToken?: string;
  }) {
    const uploadPath = this.uploadPath(options.uploadId);
    let upload: PendingUpload;
    try {
      upload = JSON.parse(
        await fs.promises.readFile(`${uploadPath}.json`, "utf-8"),
      );
    } catch {
      throw createError({
        statusCode: 400,
        message:
          "That upload doesn't exist or has expired. Upload the file again.",
      });
    }

    if (upload.arch !== options.arch)
      throw createError({
        statusCode: 400,
        message: `The uploaded file is built for ${upload.arch}, not ${options.arch}.`,
      });

    const release = await prisma.clientRelease.create({
      data: {
        tag: options.tag,
        target: upload.target,
        arch: options.arch,
        branch: options.branch,
        notes: options.notes,
        required: options.required,
        fileName: upload.fileName,
        size: BigInt(upload.size),
        sha256: upload.sha256,
        publishedAt: options.publish ? new Date() : null,
        uploaderId: options.uploaderId,
        uploaderToken: options.uploaderToken,
      },
    });

    try {
      await fs.promises.rename(uploadPath, this.releasePath(release.id));
      await fs.promises.rm(`${uploadPath}.json`, { force: true });
    } catch (e) {
      await prisma.clientRelease.deleteMany({ where: { id: release.id } });
      throw e;
    }

    return release;
  }

  async delete(id: string) {
    const { count } = await prisma.clientRelease.deleteMany({ where: { id } });
    if (count === 0) return false;
    await fs.promises.rm(this.releasePath(id), { force: true });
    return true;
  }

  /**
   * The release currently offered for each (target, arch, branch), keyed
   * "target/arch/branch"
   */
  async offeredReleases() {
    const candidates = await prisma.clientRelease.findMany({
      where: { publishedAt: { not: null }, withdrawnAt: null },
      orderBy: { publishedAt: "desc" },
    });
    const offered = new Map<string, ClientReleaseModel>();
    for (const release of candidates) {
      for (const branch of Object.keys(
        clientReleaseBranches,
      ) as ClientReleaseBranch[]) {
        if (!branchesVisibleTo(branch).includes(release.branch)) continue;
        const key = offeredKey(release.target, release.arch, branch);
        if (!offered.has(key)) offered.set(key, release);
      }
    }
    return offered;
  }

  async offeredRelease(
    target: ClientReleaseTarget,
    arch: ClientReleaseArch,
    branch: ClientReleaseBranch,
  ) {
    return await prisma.clientRelease.findFirst({
      where: {
        target,
        arch,
        branch: { in: branchesVisibleTo(branch) },
        publishedAt: { not: null },
        withdrawnAt: null,
      },
      orderBy: { publishedAt: "desc" },
    });
  }

  /** Which branches currently offer this release */
  offeredOn(
    release: ClientReleaseModel,
    offered: Map<string, ClientReleaseModel>,
  ) {
    return (Object.keys(clientReleaseBranches) as ClientReleaseBranch[]).filter(
      (branch) =>
        offered.get(offeredKey(release.target, release.arch, branch))?.id ===
        release.id,
    );
  }

  status(
    release: ClientReleaseModel,
    offered: Map<string, ClientReleaseModel>,
  ): ClientReleaseStatus {
    if (!release.publishedAt) return "draft";
    if (release.withdrawnAt) return "withdrawn";
    return this.offeredOn(release, offered).length > 0
      ? "offered"
      : "superseded";
  }

  /**
   * Works out whether a client should update, and which patch notes to show.
   * The client identifies the build it runs by release id when it knows it,
   * otherwise by the file's SHA-256 (e.g. an AppImage downloaded by hand).
   */
  async checkForUpdate(options: {
    target: ClientReleaseTarget;
    arch: ClientReleaseArch;
    branch: ClientReleaseBranch;
    currentId?: string;
    currentSha256?: string;
  }) {
    const offered = await this.offeredRelease(
      options.target,
      options.arch,
      options.branch,
    );
    if (!offered) return { update: null };

    const current = await this.resolveCurrent(options);
    if (current?.id === offered.id)
      return { update: null, current: serializeRelease(offered) };

    let notesFrom: ClientReleaseModel[] = [offered];
    let rollback = false;
    if (current?.publishedAt) {
      if (offered.publishedAt! < current.publishedAt) {
        rollback = true;
      } else {
        // Every release the client is skipping, newest first
        notesFrom = await prisma.clientRelease.findMany({
          where: {
            target: options.target,
            arch: options.arch,
            branch: { in: branchesVisibleTo(options.branch) },
            withdrawnAt: null,
            publishedAt: { gt: current.publishedAt, lte: offered.publishedAt! },
          },
          orderBy: { publishedAt: "desc" },
        });
      }
    }

    return {
      update: {
        ...serializeRelease(offered),
        rollback,
        required: notesFrom.some((release) => release.required),
        notes: notesFrom.map((release) => ({
          id: release.id,
          tag: release.tag,
          publishedAt: release.publishedAt!,
          notes: release.notes,
        })),
      },
      current: current ? serializeRelease(current) : null,
    };
  }

  private async resolveCurrent(options: {
    target: ClientReleaseTarget;
    arch: ClientReleaseArch;
    currentId?: string;
    currentSha256?: string;
  }) {
    const scope = { target: options.target, arch: options.arch };
    if (options.currentId) {
      const byId = await prisma.clientRelease.findFirst({
        where: { ...scope, id: options.currentId },
      });
      if (byId) return byId;
    }
    if (options.currentSha256) {
      // Several releases can share a file (re-uploads); the newest one wins
      return await prisma.clientRelease.findFirst({
        where: { ...scope, sha256: options.currentSha256.toLowerCase() },
        orderBy: { publishedAt: { sort: "desc", nulls: "last" } },
      });
    }
    return null;
  }
}

type PendingUpload = {
  uploadId: string;
  target: ClientReleaseTarget;
  fileName: string;
  size: number;
  sha256: string;
  arch: ClientReleaseArch;
};

/**
 * Pulls the version out of a build's file name, e.g.
 * "Drop Desktop Client_0.4.3-g91be2c0.dirty_amd64.AppImage" -> "0.4.3-g91be2c0.dirty"
 */
function suggestTag(fileName: string) {
  return fileName
    .replace(/\.AppImage$/i, "")
    .match(/\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?/)?.[0];
}

function offeredKey(
  target: ClientReleaseTarget,
  arch: ClientReleaseArch,
  branch: ClientReleaseBranch,
) {
  return `${target}/${arch}/${branch}`;
}

/** JSON can't carry BigInt */
export function serializeRelease(release: ClientReleaseModel) {
  return {
    ...release,
    size: Number(release.size),
    targetSlug: clientReleaseTargets[release.target].slug,
    branchSlug: clientReleaseBranches[release.branch],
  };
}

export const clientReleaseManager = new ClientReleaseManager();
export default clientReleaseManager;
