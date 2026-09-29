#!/usr/bin/env node
// Talks to a Drop server's Updater API for the publish-client-update skill.
//
//   node publish.mjs status
//       Local AppImages in the repo root (version, commit, dirty, sha256,
//       whether the server already has them) and the newest release per
//       branch, as JSON.
//   node publish.mjs upload <file> --tag <tag> --branch release|test
//                    --notes-file <path> [--required] [--draft]
//       Uploads one build and creates the release. Prints the release as JSON.
//   node publish.mjs token-link
//       Prints the admin page link that creates a token with the right ACLs.
//
// Config: DROP_UPDATER_URL and DROP_UPDATER_TOKEN, from the environment or
// from .claude/updater.env (gitignored by the repo's *.env rule).

import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { Readable } from "node:stream";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../../..");
const envFile = path.join(repoRoot, ".claude/updater.env");
const TOKEN_ACLS = ["updater:read", "updater:new"];

function loadConfig() {
  const config = {
    url: process.env.DROP_UPDATER_URL,
    token: process.env.DROP_UPDATER_TOKEN,
  };
  if (fs.existsSync(envFile)) {
    for (const line of fs.readFileSync(envFile, "utf-8").split("\n")) {
      const match = line.match(/^\s*(DROP_UPDATER_URL|DROP_UPDATER_TOKEN)\s*=\s*"?([^"]*)"?\s*$/);
      if (!match) continue;
      if (match[1] === "DROP_UPDATER_URL") config.url ??= match[2];
      else config.token ??= match[2];
    }
  }
  if (config.url) config.url = config.url.replace(/\/+$/, "");
  return config;
}

function fail(message, extra = {}) {
  console.log(JSON.stringify({ error: message, ...extra }, null, 2));
  process.exit(1);
}

function requireConfig(config, { token = true } = {}) {
  if (!config.url)
    fail(`DROP_UPDATER_URL isn't set. Put it in ${envFile} or the environment.`, {
      envFile,
    });
  if (token && !config.token)
    fail(`DROP_UPDATER_TOKEN isn't set. Create one with the link below and put it in ${envFile}.`, {
      envFile,
      tokenLink: tokenLink(config),
    });
}

function tokenLink(config) {
  const payload = Buffer.from(
    JSON.stringify({ name: "Claude Code updater", acls: TOKEN_ACLS }),
  ).toString("base64");
  return `${config.url ?? "<server>"}/admin/settings/tokens?payload=${encodeURIComponent(payload)}`;
}

async function api(config, method, route, init = {}) {
  const response = await fetch(`${config.url}${route}`, {
    method,
    ...init,
    headers: { Authorization: `Bearer ${config.token}`, ...init.headers },
  });
  const text = await response.text();
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    body = text;
  }
  if (!response.ok) {
    const message = body?.message || body?.statusMessage || text.slice(0, 300);
    if (response.status === 403)
      fail(`The server refused the token (HTTP 403). It needs the ${TOKEN_ACLS.join(", ")} permissions.`, {
        tokenLink: tokenLink(config),
      });
    fail(`${method} ${route} failed (HTTP ${response.status}): ${message}`);
  }
  return body;
}

function sha256(file) {
  const hash = createHash("sha256");
  const fd = fs.openSync(file, "r");
  const buffer = Buffer.alloc(4 * 1024 * 1024);
  let read;
  while ((read = fs.readSync(fd, buffer, 0, buffer.length, null)) > 0)
    hash.update(buffer.subarray(0, read));
  fs.closeSync(fd);
  return hash.digest("hex");
}

/** "Drop Desktop Client_0.4.0-g1a2b3c4.dirty_amd64.AppImage" -> parts */
function parseBuildName(name) {
  const match = name.match(/_(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)_([A-Za-z0-9]+)\.AppImage$/);
  const version = match?.[1] ?? null;
  const commit = version?.match(/-g([0-9a-f]{7,40})/)?.[1] ?? null;
  const dirty = /[.-]dirty\b/.test(version ?? name);
  return { version, commit, dirty, branch: dirty ? "test" : "release" };
}

async function status(config) {
  requireConfig(config);
  const server = await api(config, "GET", "/api/v1/admin/updater");
  const knownHashes = new Map(server.releases.map((r) => [r.sha256, r]));

  const local = fs
    .readdirSync(repoRoot)
    .filter((name) => name.endsWith(".AppImage"))
    .map((name) => {
      const file = path.join(repoRoot, name);
      const stat = fs.statSync(file);
      const hash = sha256(file);
      const existing = knownHashes.get(hash);
      return {
        file,
        ...parseBuildName(name),
        size: stat.size,
        builtAt: stat.mtime.toISOString(),
        sha256: hash,
        alreadyUploaded: existing
          ? { id: existing.id, tag: existing.tag, status: existing.status }
          : null,
      };
    })
    .sort((a, b) => b.builtAt.localeCompare(a.builtAt));

  // Newest published, non-withdrawn release per branch - the base for notes
  const latestByBranch = {};
  for (const release of server.releases) {
    if (!release.publishedAt || release.withdrawnAt) continue;
    const current = latestByBranch[release.branchSlug];
    if (!current || release.publishedAt > current.publishedAt)
      latestByBranch[release.branchSlug] = {
        id: release.id,
        tag: release.tag,
        arch: release.arch,
        publishedAt: release.publishedAt,
        commit: release.tag.match(/-g([0-9a-f]{7,40})/)?.[1] ?? null,
      };
  }

  console.log(
    JSON.stringify({ server: config.url, local, latestByBranch }, null, 2),
  );
}

function parseArgs(argv) {
  const args = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--required" || arg === "--draft") args[arg.slice(2)] = true;
    else if (arg.startsWith("--")) args[arg.slice(2)] = argv[++i];
    else args._.push(arg);
  }
  return args;
}

async function upload(config, args) {
  requireConfig(config);
  const file = args._[0];
  if (!file || !fs.existsSync(file)) fail(`No such file: ${file}`);
  if (!args.tag) fail("--tag is required");
  if (!["release", "test"].includes(args.branch))
    fail("--branch must be release or test");
  const notes = args["notes-file"]
    ? fs.readFileSync(args["notes-file"], "utf-8")
    : "";

  const size = fs.statSync(file).size;
  const query = new URLSearchParams({
    target: "linux-appimage",
    fileName: path.basename(file),
  });
  const uploaded = await api(
    config,
    "PUT",
    `/api/v1/admin/updater/upload?${query}`,
    {
      body: Readable.toWeb(fs.createReadStream(file)),
      duplex: "half",
      headers: {
        "Content-Type": "application/octet-stream",
        "Content-Length": String(size),
      },
    },
  );

  const release = await api(config, "POST", "/api/v1/admin/updater", {
    body: JSON.stringify({
      uploadId: uploaded.uploadId,
      tag: args.tag,
      arch: uploaded.arch,
      branch: args.branch,
      notes,
      required: !!args.required,
      publish: !args.draft,
    }),
    headers: { "Content-Type": "application/json" },
  });

  console.log(
    JSON.stringify(
      {
        id: release.id,
        tag: release.tag,
        branch: release.branchSlug,
        arch: release.arch,
        sha256: release.sha256,
        published: !!release.publishedAt,
        page: `${config.url}/admin/settings/updater/${release.id}`,
      },
      null,
      2,
    ),
  );
}

const [command, ...rest] = process.argv.slice(2);
const config = loadConfig();
switch (command) {
  case "status":
    await status(config);
    break;
  case "upload":
    await upload(config, parseArgs(rest));
    break;
  case "token-link":
    requireConfig(config, { token: false });
    console.log(tokenLink(config));
    break;
  default:
    console.error("usage: publish.mjs status | upload <file> --tag T --branch B --notes-file F [--required] [--draft] | token-link");
    process.exit(2);
}
