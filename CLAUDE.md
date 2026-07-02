# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

This is the main [Drop](https://droposs.org) monorepo — a self-hosted, DRM-free game distribution platform (think a homelab-friendly Steam/GameVault alternative). It's a pnpm workspace containing the server, the desktop client, a CLI, and several shared libraries. **Builds must run inside a Dockerfile, not on the host** — see the root `Dockerfile` and the per-package notes below; only lightweight verification (typecheck/lint/`cargo check`) is fine to run directly on the host.

## Workspace layout

Declared in `pnpm-workspace.yaml`: `server/`, `libraries/base/`, `sites/*`, `desktop/`.

- **`server/`** — the actual Drop server: a Nuxt 3 full-stack app (Nitro backend + Vue frontend) backed by Postgres via Prisma. This is where almost all backend work happens.
- **`desktop/`** — the Tauri 2 desktop client (Rust + Nuxt 3, package name `drop-app`). This is the upstream copy of the same app maintained as its own repo/fork elsewhere — the two can diverge, so don't assume `desktop/` here is always in sync with a separate `drop-app` checkout.
- **`cli/`** (crate `downpour`) — an admin CLI for tasks that need local/direct access, notably uploading game content to a depot (S3 or local). See `cli/spec.md` and `cli/README.md`.
- **`torrential/`** — a Rust sidecar webserver that serves game content files at high speed; spawned as a child process by the Nuxt server and reverse-proxied by nginx at `/api/v1/depot`. Not a pnpm workspace member (it's a standalone Cargo crate).
- **`libraries/base/`** — shared Nuxt UI layer (components/composables) extended by both `server/` and `desktop/` via `nuxt.config.ts`'s `extends`.
- **`libraries/droplet`** (crate `droplet-rs`) — shared Rust utilities used by the server (via node bindings/protobuf, not directly linked), the desktop client, `cli`, and `torrential` — manifest generation and version-backend read/write abstractions live here.
- **`libraries/droplet_types`** — types shared between the desktop client and `droplet-rs`, split out so the client doesn't need to compile all of `droplet-rs`.
- **`libraries/libarchive`**, **`libraries/native_model`** — vendored/forked third-party crates (not Drop-specific business logic).
- **`sites/docs`** (Astro) and **`sites/promo`** (Next.js) — the docs site and marketing site; largely independent of the rest of the stack.

Root `proto/`-adjacent protobuf definitions live in `torrential/proto/*.proto` and are compiled into TypeScript for the server via `buf` (`server/buf.gen.yaml`, output into `server/server/internal/proto`) and into Rust for `torrential`/`droplet-rs` directly.

## Commands

### Root
- Install all workspace deps: `pnpm install`

### Server (`server/`, run with `pnpm --filter=drop <script>` or `cd server && pnpm <script>`; the package name is `drop`)
- Dev: `pnpm dev` (needs Postgres — `docker compose -f server/dev-tools/compose.yml up` for a local instance)
- Build: `pnpm build` (runs `nuxt build`; `postinstall` already ran `prisma generate` + `buf generate`)
- Typecheck: `pnpm run typecheck`
- Lint: `pnpm run lint` (= `lint:eslint` + `lint:prettier`); autofix with `pnpm run lint:fix`
- Prisma schema is split across `server/prisma/models/*.prisma`; after changing it, re-run `prisma generate` (via `postinstall`) and create a migration before relying on the client types.

### Desktop (`desktop/`)
Same shape as a standalone Tauri 2 + Nuxt 3 app — see that project's own CLAUDE.md/README for exact commands (`pnpm tauri dev`, `pnpm tauri build`, `cargo clippy --manifest-path ./desktop/src-tauri/Cargo.toml`, nightly Rust toolchain pinned in `desktop/src-tauri/rust-toolchain.toml`).

### Rust crates (`cli/`, `torrential/`, `libraries/droplet*`)
- Build/check: `cargo build`/`cargo check --manifest-path <crate>/Cargo.toml`
- Format/lint/test (pattern used in CI for `libraries/droplet`, applies the same way to the others): `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features --all --verbose`
- `cli` and `desktop/src-tauri` pin Rust **nightly** via `rust-toolchain.toml`; `torrential`/`libraries/droplet*` don't pin a file but CI still uses nightly — match that locally.
- `torrential` needs `libarchive-dev` (or equivalent) installed to link `libarchive` for `libarchive3-sys`.

### Sites
- `sites/docs`: Astro (`astro dev`/`astro build`)
- `sites/promo`: Next.js (`next dev`/`next build`)

## Architecture

### Server request handling
Nitro file-based routing under `server/server/api/v1/**` — the HTTP method is encoded in the filename (`index.get.ts`, `setup.post.ts`, etc.), each exporting `defineEventHandler`. Non-API routes (e.g. auth callbacks) live in `server/server/routes/`.

### Permissions (ACLs)
Access control is a flat, colon-namespaced string list, not a role table. `server/server/internal/acls/index.ts` defines two arrays — `userACLs` (e.g. `"object:read"`, `"collections:new"`) and `systemACLs` (admin-level, e.g. `"depot:read"`, `"setup"`) — checked per-request via `aclManager.getUserACL(h3, [...])`/`hasACL(...)` for user-level permissions and `aclManager.allowSystemACL(h3, [...])` for admin-level ones (see `admin/index.get.ts` for a minimal example). When adding a new API route or capability, add the permission string to the relevant array in that file first.

### Prisma — never call `.delete()`/`.update()` directly
There's a **custom ESLint rule** (`server/rules/no-prisma-delete.mts`, wired into `server/eslint.config.mjs`) that forbids `prisma.<model>.delete()`/`.update()`. Always use `.deleteMany()`/`.updateMany()` and check the returned count instead — `lint` will fail otherwise. This exists because the singular forms throw on "not found" in a way that's easy to mishandle; the plural forms let you check `count` explicitly.

### Object storage abstraction
`server/server/internal/objects/objectHandler.ts` defines `ObjectBackend` (currently backed by `fsBackend.ts`, a filesystem implementation) — a generic "objects are files with metadata + string-based permissions" abstraction (`id:permission` entries like `anonymous:read`) served from `/api/v1/object/:objectId`. Each logical thing (a user's avatar, a screenshot, etc.) gets a single object ID that's overwritten in place on update, rather than versioned.

### Sidecar services (nginx + torrential)
The Nuxt server doesn't just serve HTTP — on boot it spawns and supervises child processes via `server/server/internal/services` (`ServiceManager`/`Service` classes handle relaunch-on-crash + periodic healthchecks):
- **nginx** (`services/services/nginx.ts`) — reverse proxy in front of the Node process and the depot.
- **torrential** (`services/torrential/`) — the Rust content server. The Node server talks to it over a raw TCP socket using length-prefixed protobuf messages (`DropBound`/`TorrentialBound` envelope types generated from `torrential/proto/core.proto`), not HTTP — see `services/torrential/index.ts`. A service can be disabled/pointed elsewhere in dev via `EXTERNAL_SERVICE_<NAME>`-style env vars (checked in `Service.launch()`).

### Library format
Game content on disk follows `/{game name}/{version name}/...` (see `server/server/internal/library/README.md`); the game name is just for initial matching (actual identity is the DB entry), and versions/deltas are configured manually in the web UI. `server/server/internal/library/providers/` (`filesystem.ts`, `flat.ts`) implement how that tree is discovered. The `downpour` CLI (`cli/`) is the tool for populating a depot with game content in this format (`new`/`upload`/`copy`/`mark` subcommands — see `cli/spec.md`).

### Deployment shape
The root `Dockerfile` is the source of truth for how a production instance is assembled: it builds `torrential` in a separate Rust stage, builds the Nuxt server (`pnpm run --filter=drop postinstall && pnpm run --filter=drop build`), then in the runtime image installs nginx + the `torrential` binary + Prisma CLI and runs `server/build/launch.sh`, which does `prisma migrate deploy` before starting `node .../server/index.mjs`. Runtime config is env-var driven (`LIBRARY`, `DATA`, `EXTERNAL_URL`, `NGINX_CONFIG`, `INTERNAL_DEPOT_URL`, `OIDC_REQUIRE_HTTPS`, `METADATA_TIMEOUT`, `PORT`) — see `server/server/internal/config/sys-conf.ts` for the full set and defaults.
