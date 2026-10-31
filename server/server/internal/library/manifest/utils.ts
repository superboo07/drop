import type { JsonValue } from "@prisma/client/runtime/client";

export type DropletManifest = V2Manifest;

export type V2Manifest = {
  version: "2";
  size: number;
  key: number[];
  chunks: { [key: string]: V2ChunkData };
  /***
   * Whole-file SHA-256 (hex) per relative filename, independent of chunk
   * boundaries. Optional since manifests generated before this field existed
   * won't have it.
   */
  fileHashes?: { [filename: string]: string };
};

export type V2ChunkData = {
  files: Array<V2FileEntry>;
  checksum: string;
  iv: number[];
};

export type V2FileEntry = {
  filename: string;
  start: number;
  length: number;
  permissions: number;
  /***
   * The target of a symbolic link, verbatim, when this entry is one. Game
   * files are carried as they are, so a link has no content of its own
   * (`length` is 0) and is recreated as a link rather than as a copy of
   * whatever it points at. Absent on every other entry.
   */
  symlink?: string;
};

export function castManifest(manifest: JsonValue): DropletManifest {
  return JSON.parse(manifest as string) as DropletManifest;
}
