import { DateTime } from "luxon";

export type ClientReleaseStatus =
  | "draft"
  | "offered"
  | "superseded"
  | "withdrawn";

export const clientReleaseStatusStyles: Record<
  ClientReleaseStatus,
  { label: string; class: string }
> = {
  offered: {
    label: "Offered",
    class: "bg-green-400/10 text-green-400 ring-green-400/20",
  },
  draft: {
    label: "Draft",
    class: "bg-yellow-400/10 text-yellow-400 ring-yellow-400/20",
  },
  superseded: {
    label: "Superseded",
    class: "bg-zinc-400/10 text-zinc-400 ring-zinc-400/20",
  },
  withdrawn: {
    label: "Withdrawn",
    class: "bg-red-400/10 text-red-400 ring-red-400/20",
  },
};

export type ClientReleaseBranchSlug = "release" | "test";

export const clientReleaseBranchStyles: Record<
  ClientReleaseBranchSlug,
  { label: string; class: string }
> = {
  release: {
    label: "Release",
    class: "bg-zinc-400/10 text-zinc-300 ring-zinc-400/20",
  },
  test: {
    label: "Test",
    class: "bg-purple-400/10 text-purple-300 ring-purple-400/20",
  },
};

export function formatReleaseSize(bytes: number) {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function formatReleaseDate(date: string | Date) {
  const value =
    typeof date === "string"
      ? DateTime.fromISO(date)
      : DateTime.fromJSDate(date);
  return value.toLocaleString(DateTime.DATETIME_MED);
}
