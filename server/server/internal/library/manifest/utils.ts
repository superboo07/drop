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
};

export function castManifest(manifest: JsonValue): DropletManifest {
  return JSON.parse(manifest as string) as DropletManifest;
}
