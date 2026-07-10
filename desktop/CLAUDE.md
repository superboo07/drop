# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

Drop Desktop Client — the desktop app for [Drop](https://github.com/Drop-OSS/drop), a self-hosted game distribution platform. It's a Tauri 2 app: Rust backend, Nuxt 3 + TailwindCSS frontend (chosen specifically so UI components can be shared with Drop's separate web client).

## Setup

This repo uses git submodules — always run this after cloning or pulling submodule changes:
```
git submodule update --init --recursive
```
`libs/drop-base` (shared Nuxt UI layer/components) and `src-tauri/tailscale/libtailscale` are submodules. `libs/drop-base` is required for the build to succeed at all (`build.mjs` checks for it and throws if missing). `src-tauri/tailscale` is vendored but currently not wired into the workspace/build — it's not a dependency of anything and isn't compiled by a normal build.

## Commands

- Install deps: `pnpm install` (root) — this only installs the root/Tauri CLI deps; `build.mjs` handles installing each frontend view's own deps.
- Dev: `pnpm tauri dev` (NVIDIA/Linux users: use `./nvidia-prop-dev.sh` instead, which sets `GSK_RENDERER=ngl`)
- Build: `pnpm tauri build` (runs `beforeBuildCommand: pnpm build` first, which runs `build.mjs`)
- Frontend-only build: `pnpm build` (runs `build.mjs`, which installs deps and runs `nuxt generate` for each view directory under repo root that has a `package.json`, currently just `main/`, copying output into `.output/<view>`)
- Frontend typecheck: `pnpm -C main typecheck` (or `pnpm --prefix main typecheck`)
- Rust lint: `cargo clippy --manifest-path ./src-tauri/Cargo.toml` (this is what CI runs, on Rust nightly)
- Logging: set `RUST_LOG=[debug,info,warn,error]` env var, e.g. `RUST_LOG=debug pnpm tauri dev`

Rust toolchain is pinned to **nightly** (`src-tauri/rust-toolchain.toml`) — several crates rely on unstable features (`nonpoison_mutex`, `iterator_try_collect`, etc).

This workspace has **no `[workspace.dependencies]` table** — don't add `dep = { workspace = true }` to a sub-crate's `Cargo.toml`, it will fail to resolve. Each crate declares its own dependency versions directly (see any sub-crate's `Cargo.toml` for the pattern, e.g. `process/Cargo.toml`'s `uuid = "1.18.1"`), even when the same crate/version is already a dependency elsewhere in the workspace.

`cargo check`/`clippy` on a single sub-crate in isolation (`cargo check -p database`) can show spurious errors — e.g. missing derive macros — that don't reflect a real problem, because feature unification across the workspace (e.g. `serde`'s `derive` feature, enabled by another crate) isn't applied. Always verify against the full workspace manifest (`cargo check --manifest-path ./src-tauri/Cargo.toml`, or `cargo clippy` per the Commands section above) before concluding something is broken.

**Builds (not lint/typecheck) must run inside a Dockerfile, not on the host.** Use `bash build_appimage.sh` (`Dockerfile.build`) for actual build artifacts rather than installing/running the Rust or Node toolchains directly on the host. This applies to Drop's other repo (the server) too. Lightweight verification of an edit — `cargo check`, `cargo clippy`, `pnpm -C main typecheck` — is fine to run on the host if the toolchain is already there.

**Never run `nuxt dev`/`pnpm -C main dev` on the host to eyeball a page.** It writes into `main/.nuxt` and can leave `main/.output/public` with dev-mode HTML (`@vite/client` script tags, absolute host `node_modules` paths instead of hashed prod assets) — this happened once (leftover dev server from a verification step corrupted the AppImage's main window, which then failed to load with `AssetNotFound` errors). `build.mjs` copies `main/.output/public` straight into the AppImage/bundle with no sanity check, so this silently ships. If you must run a dev server for verification, kill it and confirm with `ps aux | grep nuxt` (or equivalent) that it's actually gone — don't trust a bare `pkill` exit code — then `rm -rf main/.nuxt main/.output .output` before the next real build.

### AppImage builds (Linux)

`bash build_appimage.sh` builds a Docker image from `Dockerfile.build` and runs the whole build inside a container (no host Rust/Node install needed). See "AppImage gotchas" below before touching anything that spawns subprocesses or affects the Linux bundle target.

## Architecture

### Repo layout

- `main/` — the Nuxt 3 frontend (the only "view" currently; `build.mjs` is written to support multiple views if more get added). Extends `libs/drop-base` via `nuxt.config.ts`'s `extends`, which is where shared components/composables (e.g. the modal stack, `createModal`/`useModalStack` in `libs/drop-base/composables/modal-stack.ts`) live. SSR is disabled (it's a Tauri webview, not a server).
- `src-tauri/` — the Rust backend. `src-tauri/src/` is the actual Tauri app crate (binary + commands); the rest are internal library crates in a Cargo workspace:
  - `client` — app status/state, autostart, platform compat helpers
  - `database` — the local persisted DB (see below)
  - `download_manager` — game download queueing, pausing/resuming, progress
  - `games` — game library, collections, install/version management
  - `process` — launching and monitoring game processes (Proton/Wine/umu-run on Linux, native elsewhere), including compat-layer (Proton path) management
  - `remote` — talks to the user's Drop server: auth, object fetching, the custom `server://` URI scheme proxy
  - `cloud_saves` — cloud save backup/sync
  - `utils` — small shared helpers (`app_emit!` macro, locking helpers)

### Frontend/backend bridge

Tauri commands are registered in the big `generate_handler![...]` call in `src-tauri/src/lib.rs`; the frontend calls them with `invoke()` from `@tauri-apps/api/core`. Backend → frontend events are emitted via the `app_emit!` macro (`utils` crate) and consumed in the frontend with `listen()` from `@tauri-apps/api/event` — the central place wiring most of these up is `main/composables/state-navigation.ts`.

### Persistence

There's a single local, AES-CTR–encrypted database file (`drop.db`, or the whole data dir is `drop-debug` in debug builds) under the OS data dir, not a set of separate config files. It's defined by `database::interface::DatabaseInterface` wrapping `database::models::data::Database`, guarded by an `RwLock`. Read/write access from anywhere else in the backend goes through `database::{borrow_db_checked, borrow_db_mut_checked}` — never construct/lock it directly. User-facing settings live in the `Settings` struct inside `Database` (`database/src/models.rs`), exposed via the `fetch_settings`/`update_settings` commands (`src-tauri/src/settings.rs`); `update_settings` does a merge (JSON round-trip over the existing struct, patched with whatever fields the frontend sent), so adding a new setting means: add the field to `Settings` (with `#[serde(default)]` so existing on-disk databases without it still deserialize), add it to the `Default` impl, add it to the frontend `Settings` type in `main/types.ts`.

### Client capabilities (server-gated features)

Features that need server support (cloud saves, playtime tracking, etc.) are gated by a capability string (`peerAPI`, `cloudSaves`, `trackPlaytime`, ...) that the server records per-`Client` row. The set of capabilities a client has is **not** re-derived from the app version — it's whatever was explicitly requested and got recorded server-side, and by default that only happens once, during the initial pairing handshake (`auth_initiate_logic` in `remote/src/auth.rs`, which sends a fixed capabilities list to `/api/v1/client/auth/initiate`).

This means: **adding a new capability-gated feature does nothing for already-paired clients** — their `Client.capabilities` array on the server was written before the feature existed and never changes on its own. Every request from that device 403s, and that failure is easy to miss (the sync/fetch code that hits a capability-gated endpoint typically just logs a warning and quietly no-ops, per the "everything stays queued, retry next cycle" pattern used by scheduled sync tasks — nothing surfaces to the user). This exact bug shipped once with playtime tracking: implemented correctly end-to-end, but silently never worked for any device paired before the feature landed.

The fix/pattern: call `remote::auth::request_capability("theCapability")` (hits the separate `/api/v1/client/capability` endpoint, which upserts onto an existing `Client` row) somewhere that runs for already-paired installs too — `setup()` in `remote/src/auth.rs` does this on every app launch for `trackPlaytime`. The server-side upsert is idempotent (no-ops if the client already has the capability), so it's safe to call unconditionally on every startup rather than trying to track "have we already requested this." When adding a new capability-gated feature, add its capability request here too, not just to the pairing handshake's list.

### Window/tray behavior

The main window's close button hides to the system tray by default rather than quitting (`on_window_event` + the `RunEvent::ExitRequested` handler in `src-tauri/src/lib.rs`, both gated through the `run_on_tray` helper). This is controlled by the `quit_on_close` setting — both the close-requested and the subsequent exit-requested handlers need to agree, or you get a window that closes but leaves a zombie tray process (that was a real bug — see git history).

### AppImage gotchas (Linux)

The AppImage build has bitten several real bugs, all from the same root cause: linuxdeploy's `AppRun` sets `LD_LIBRARY_PATH`, `PATH`, `PYTHONHOME`, and `PYTHONPATH` to point at the bundle's own libs, and these leak into *every* subprocess the app spawns unless explicitly stripped:
- `open_process_logs` (`src-tauri/src/process.rs`) bypasses the bundled (older) `xdg-open` by calling `/usr/bin/xdg-open` directly with `LD_LIBRARY_PATH` removed — the bundled xdg-utils version is old enough to not know Plasma 6's `kde-open` naming, and the polluted `LD_LIBRARY_PATH` can also break whatever native file-manager helper it execs.
- Game/launcher processes (`process::process_manager::launch_process`) explicitly `env_remove` `PYTHONHOME`, `PYTHONPATH`, and `LD_LIBRARY_PATH` before spawning — without this, Python-based launchers like `umu-run` crash with `Fatal Python error: Failed to import encodings module` because they inherit a `PYTHONHOME` pointing into the (Python-less) AppImage bundle.
- `Dockerfile.build` pins the build's `libwebkit2gtk-4.1`/`libjavascriptcoregtk-4.1` to 2.44.2 (via Ubuntu 23.10/mantic, since it's EOL apt is redirected to `old-releases.ubuntu.com`) — every WebKitGTK release past 2.44 has an unresolved upstream EGL/Wayland regression that aborts the WebProcess on launch, and versions before that lack API symbols `wry`/Tauri need to link.

If you add new code that spawns an external process on Linux, assume it will inherit this pollution and strip what's irrelevant to that process, the same way the two examples above do.
