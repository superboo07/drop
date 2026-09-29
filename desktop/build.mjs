import fs from "fs";
import process from "process";
import childProcess from "child_process";
import crypto from "crypto";
import createLogger from "pino";

const OUTPUT = "./.output";
const logger = createLogger({ transport: { target: "pino-pretty" } });

async function spawn(exec, opts) {
  const output = childProcess.spawn(exec, { ...opts, shell: true });
  output.stdout.on("data", (data) => {
    process.stdout.write(data);
  });
  output.stderr.on("data", (data) => {
    process.stderr.write(data);
  });

  return await new Promise((resolve, reject) => {
    output.on("error", (err) => reject(err));
    output.on("exit", () => resolve());
  });
}

const views = fs.readdirSync(".").filter((view) => {
  const expectedPath = `./${view}/package.json`;
  return fs.existsSync(expectedPath);
});

fs.mkdirSync(OUTPUT, { recursive: true });

// Hash of everything a view's build output depends on: its own files, the
// shared layer it extends (libraries/base), both lockfiles and this script.
// Tracked and untracked-but-not-ignored files, so node_modules/.nuxt/.output
// don't count. Read through git, which the build container has.
function inputsHash(view) {
  const files = childProcess
    .execFileSync(
      "git",
      [
        "ls-files",
        "-z",
        "-co",
        "--exclude-standard",
        "--",
        view,
        "../libraries/base",
      ],
      { encoding: "utf8" },
    )
    .split("\0")
    .filter((file) => file && fs.existsSync(file))
    .concat(["../pnpm-lock.yaml", "build.mjs"])
    .sort();
  const hash = crypto.createHash("sha256");
  hash.update(`NUXT_APP_BASE_URL=/${view}/\n`);
  for (const file of files) {
    hash.update(`${file}\0`);
    hash.update(fs.readFileSync(file));
  }
  return hash.digest("hex");
}

for (const view of views) {
  const loggerChild = logger.child({});

  // Skip the view entirely when nothing it's built from has changed. Not
  // just to save the Nuxt build: tauri embeds every file under OUTPUT with
  // include_bytes!, so rewriting them - even byte-identical - bumps their
  // mtimes and makes cargo recompile (and re-LTO) drop-app for nothing.
  const stampPath = `./${view}/.output/.drop-build-inputs`;
  const hash = inputsHash(view);
  if (
    fs.existsSync(`${OUTPUT}/${view}`) &&
    fs.existsSync(stampPath) &&
    fs.readFileSync(stampPath, "utf8") === hash
  ) {
    loggerChild.info(`"${view}" is up to date, not rebuilding it`);
    continue;
  }

  process.chdir(`./${view}`);

  loggerChild.info(`Install deps for "${view}"`);
  // In the AppImage build container there's no TTY for pnpm to prompt on,
  // so CI mode makes it answer non-interactively. Only for this command:
  // this script runs inside `tauri build`, whose cargo must not see CI (it
  // turns incremental compilation off). See build_appimage.sh.
  await spawn("pnpm install", {
    env: process.env.DROP_IN_DOCKER
      ? { ...process.env, CI: "true" }
      : process.env,
  });

  loggerChild.info(`Building "${view}"`);
  await spawn("pnpm run build", {
    env: { ...process.env, NUXT_APP_BASE_URL: `/${view}/` },
  });

  process.chdir("..");

  // Replace, not merge: a merge would keep every stale hashed asset from
  // earlier builds and embed them all into the binary.
  fs.rmSync(`${OUTPUT}/${view}`, { recursive: true, force: true });
  fs.cpSync(`./${view}/.output/public`, `${OUTPUT}/${view}`, {
    recursive: true,
  });
  fs.writeFileSync(stampPath, hash);
}
