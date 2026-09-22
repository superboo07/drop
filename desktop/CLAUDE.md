# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

Drop Desktop Client — the desktop app for [Drop](https://github.com/Drop-OSS/drop), a self-hosted game distribution platform. It's a Tauri 2 app: Rust backend, Nuxt 3 + TailwindCSS frontend (chosen specifically so UI components can be shared with Drop's separate web client).

## Setup

This app lives at `desktop/` inside the Drop monorepo. The shared Nuxt UI layer/components it used to pull in as the `libs/drop-base` submodule is now a sibling directory in the same repo, `libraries/base`, which `main/nuxt.config.ts` extends via a relative path (`../../libraries/base`) — so anything that builds this app needs the whole repo present, not just `desktop/`. The only submodules are the nested session's tools under `desktop/vendor/` (`git submodule update --init desktop/vendor`); only the Linux AppImage build needs them.

## Commands

- Install deps: `pnpm install --filter drop-app` from the *monorepo* root — `desktop/` is a workspace member, so the install runs from one level up. This only installs the Tauri CLI deps; `build.mjs` handles installing each frontend view's own deps.
- Dev: `pnpm tauri dev` (NVIDIA/Linux users: use `./nvidia-prop-dev.sh` instead, which sets `GSK_RENDERER=ngl`)
- Build: `pnpm tauri build` (runs `beforeBuildCommand: pnpm build` first, which runs `build.mjs`)
- Frontend-only build: `pnpm build` (runs `build.mjs`, which installs deps and runs `nuxt generate` for each view directory under `desktop/` that has a `package.json`, currently just `main/`, copying output into `.output/<view>`)
- Frontend typecheck: `pnpm -C main typecheck` (or `pnpm --prefix main typecheck`)
- Rust lint: `cargo clippy --manifest-path ./src-tauri/Cargo.toml` (this is what CI runs, on Rust nightly)
- Logging: set `RUST_LOG=[debug,info,warn,error]` env var, e.g. `RUST_LOG=debug pnpm tauri dev`

Rust toolchain is pinned to **nightly** (`src-tauri/rust-toolchain.toml`) — several crates rely on unstable features (`nonpoison_mutex`, `iterator_try_collect`, etc).

This workspace has **no `[workspace.dependencies]` table** — don't add `dep = { workspace = true }` to a sub-crate's `Cargo.toml`, it will fail to resolve. Each crate declares its own dependency versions directly (see any sub-crate's `Cargo.toml` for the pattern, e.g. `process/Cargo.toml`'s `uuid = "1.18.1"`), even when the same crate/version is already a dependency elsewhere in the workspace.

`cargo check`/`clippy` on a single sub-crate in isolation (`cargo check -p database`) can show spurious errors — e.g. missing derive macros — that don't reflect a real problem, because feature unification across the workspace (e.g. `serde`'s `derive` feature, enabled by another crate) isn't applied. Always verify against the full workspace manifest (`cargo check --manifest-path ./src-tauri/Cargo.toml`, or `cargo clippy` per the Commands section above) before concluding something is broken.

**Builds (not lint/typecheck) must run inside a Dockerfile, not on the host.** Use `bash desktop/build_appimage.sh` (`desktop/Dockerfile.build`) for actual build artifacts rather than installing/running the Rust or Node toolchains directly on the host. This applies to the server side of the monorepo too. Lightweight verification of an edit — `cargo check`, `cargo clippy`, `pnpm -C main typecheck` — is fine to run on the host if the toolchain is already there.

**Never run `nuxt dev`/`pnpm -C main dev` on the host to eyeball a page.** It writes into `main/.nuxt` and can leave `main/.output/public` with dev-mode HTML (`@vite/client` script tags, absolute host `node_modules` paths instead of hashed prod assets) — this happened once (leftover dev server from a verification step corrupted the AppImage's main window, which then failed to load with `AssetNotFound` errors). `build.mjs` copies `main/.output/public` straight into the AppImage/bundle with no sanity check, so this silently ships. If you must run a dev server for verification, kill it and confirm with `ps aux | grep nuxt` (or equivalent) that it's actually gone — don't trust a bare `pkill` exit code — then `rm -rf main/.nuxt main/.output .output` before the next real build.

### AppImage builds (Linux)

`bash desktop/build_appimage.sh` builds a Docker image from `desktop/Dockerfile.build` and runs the whole build inside a container (no host Rust/Node install needed). It mounts the monorepo root, since both the pnpm workspace root and `libraries/base` live above `desktop/`. The finished AppImage is written to the repo root. See "AppImage gotchas" below before touching anything that spawns subprocesses or affects the Linux bundle target.

## Architecture

### Repo layout

- `main/` — the Nuxt 3 frontend (the only "view" currently; `build.mjs` is written to support multiple views if more get added). Extends `libraries/base` (at the monorepo root) via `nuxt.config.ts`'s `extends`, which is where shared components/composables (e.g. the modal stack, `createModal`/`useModalStack` in `libraries/base/composables/modal-stack.ts`) live. SSR is disabled (it's a Tauri webview, not a server).
- `src-tauri/` — the Rust backend. `src-tauri/src/` is the actual Tauri app crate (binary + commands); the rest are internal library crates in a Cargo workspace:
  - `client` — app status/state, autostart, platform compat helpers
  - `database` — the local persisted DB (see below)
  - `download_manager` — game download queueing, pausing/resuming, progress
  - `games` — game library, collections, install/version management
  - `process` — launching and monitoring game processes (Proton/Wine/umu-run on Linux, native elsewhere), including compat-layer (Proton path) management and the LunaTranslator integration (`process/src/luna.rs`)
  - `nested_session` — a nested X server running bundled openbox/picom/tint2, for running a game and LunaTranslator as draggable, composited windows on one surface (Linux only; see below)
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

### LunaTranslator + nested sessions (Linux)

Windows games can be launched with [LunaTranslator](https://github.com/HIllya51/LunaTranslator) attached, hooking their text out of the Proton prefix as they're played. Two per-game toggles in `UserConfiguration` drive it (`lunaTranslator`, `nestedSession`), set from the Proton tab of a game's options; the global setup (which LunaTranslator to run, where the bridge is, which port) lives in `Settings` and `pages/settings/translation.vue`.

Drop never bundles or links any LunaTranslator code — it's GPL, Drop's desktop client is AGPL, and the only thing that would make that a question is linking. What actually happens is: the user supplies their own copy, we run it as an ordinary child process, and the two halves talk over TCP on loopback.

The bridge is upstream's `LunaCompanion.exe`: a Windows executable that runs *inside* the game's prefix, injects LunaHook into the game, and connects back out to LunaTranslator on the host (LunaTranslator is the server, the companion is the client, and the companion retries for ~30s — so launch ordering is forgiving). `UMUCompatLauncher` wraps the game command in it, which is the only change to the launch string:

```
umu-run LunaCompanion.exe 127.0.0.1:52300 --32 --hook-dir 'Z:\...\LunaHook\' 'Z:\...\game.exe' [args]
```

Three things about that are easy to get wrong:
- `--hook-dir` is **mandatory** — the companion exits immediately without it.
- The game path must be a *Wine* path (`Z:` + backslashes), because the companion `CreateProcess`es it from inside the prefix. The companion itself stays a host path, since Proton is what runs it.
- The companion defaults to its own bitness (64), and can't detect the game's. `pe_is_64_bit` in `process/src/luna.rs` reads the game's PE header to pass `--32`/`--64`, which matters because visual novels — the reason this integration exists at all — are overwhelmingly 32-bit.

The bridge ships inside the LunaTranslator AppImage, so Drop extracts its own copy once (`<appimage> --appimage-extract 'opt/lunatranslator/files/LunaHook/*'`, a couple of MB rather than the full ~360MB image) into `<data dir>/luna/LunaHook`. The `lunaBridgePath` setting is an override for people who have it elsewhere, not something anyone needs to fill in.

**The nested session** exists for the Steam Deck's game mode: gamescope composites exactly one window, so a game and a translator beside it are two things you can only ever look at one at a time. `nested_session` starts a rootful, fullscreen `Xwayland` (or `Xephyr` on an X11 desktop) — which the host compositor sees as one ordinary window, i.e. exactly the one window gamescope will show — and runs three off-the-shelf tools inside it:

- **openbox** — floating window manager. Deliberately **floating, not tiling**: on a controller/trackpad, windows the user positions beat any layout the WM picks. It already handles everything games and Qt need (EWMH fullscreen, `WM_NORMAL_HINTS`, Motif hints, `WM_TAKE_FOCUS`, `_NET_WM_MOVERESIZE`, `_NET_FRAME_EXTENTS`, transients, iconify), and frames a 32-bit client in a 32-bit frame so its alpha survives.
- **picom** — compositor. X11 has no alpha blending without one, and Qt (so LunaTranslator) won't even *try* to draw a translucent window until something owns `_NET_WM_CM_S<screen>`. xrender backend (built without OpenGL); inside Xwayland that's still GPU work via glamor.
- **tint2** — taskbar. A fullscreen Windows game minimises *itself* whenever it loses activation (`WM_ACTIVATEAPP` under Wine — not a bug), and a Deck in game mode has no keyboard, so without a taskbar minimise silently *loses windows*. Click = `toggle` (focus or restore, never minimise). Its strut shrinks `_NET_WORKAREA`, so maximized windows stop above it.

**We didn't write a window manager.** An earlier version of this crate was a ~3000-line hand-written reparenting WM (it's in git history, commit `093b8532`). It was replaced when transparency needed a compositor too — maintaining our own WM *and* compositor made no sense when mature ones exist. Don't reintroduce one.

**Where they come from.** Submodules under `desktop/vendor/` (openbox `release-3.6.1`, picom `v13`, tint2 `v17.0.2`, plus libconfig for picom since mantic's is too old), built by `bash desktop/vendor/build.sh` — in Docker (`desktop/vendor/Dockerfile`, same `ubuntu:23.10` base as `Dockerfile.build` so glibc matches), from a *copy* of the sources so the submodules stay clean. Output is a relocatable prefix at `desktop/vendor/out/nested-session/{bin,lib,licenses}`: their libraries bundled, `RUNPATH $ORIGIN/../lib`, with only libc/X11/font/GL taken from the host (AppImage's excludelist split). `build_appimage.sh` runs that script first and copies the prefix into the AppImage at `usr/libexec/drop-tools/nested-session`. Licensing: each is a separate process (GPL-2 openbox/tint2, MPL/MIT picom, LGPL libconfig statically inside picom) — nothing links into Drop.

**Drop's openbox patches** (`desktop/vendor/patches/openbox-*.patch`, applied to the build copy):
- `fullscreen-layer`: upstream lifts a *focused* fullscreen window above `_NET_WM_STATE_ABOVE` windows and docks. That would hide LunaTranslator's always-on-top window and the taskbar behind a fullscreen game — the exact case this feature exists for. Patched so fullscreen windows stay in their normal layer.
- `fullscreen-configure`: a move/resize request from a *fullscreen* window is remembered as its windowed geometry (upstream discards it and later restores the pre-fullscreen size). Wine sends the windowed size *before* dropping `_NET_WM_STATE_FULLSCREEN`, so without this a game that started fullscreen and switched to windowed stayed screen-sized — rendering scaled, with every click offset. Requests that just restate the monitor size are ignored.
- `min-label-height`: adds a `window.label.height` theme key. Upstream sizes titlebar buttons from the font height alone; this gets 30px buttons (34px titlebar) for the Deck's trackpad cursor without a giant font.

**Config** lives in `src-tauri/nested_session/config/` (`rc.xml`, `menu.xml`, `picom.conf`, `tint2rc`, `theme/` — themerc + 14px XBM button glyphs), compiled in with `include_str!` and written to `$XDG_RUNTIME_DIR/drop-nested-session-<pid>-<display>` per session. `src/tools.rs` finds the prefix (`$DROP_NESTED_SESSION_TOOLS`, then `$APPDIR/usr/libexec/drop-tools/nested-session`, then — debug builds only — `desktop/vendor/out/nested-session` in the checkout), starts them in order and only calls each one started once it's visibly working (openbox: `_NET_SUPPORTING_WM_CHECK`; picom: owns `_NET_WM_CM_S0`; tint2: `_NET_WORKAREA` shrunk). Their stdout/stderr go to Drop's log prefixed `[openbox]`/`[picom]`/`[tint2]`, since there's no terminal in game mode. A tool that dies is restarted (up to 5 times a minute).

**Everything is spawned from one dedicated `nested-session` thread**, which also supervises and tears down. That's not style: `PR_SET_PDEATHSIG` (`server::die_with_parent`) fires when the *thread* that forked the child exits, not the process — spawning from the caller's thread (possibly a pooled worker) would kill the X server whenever that thread happened to be retired.

Both LunaTranslator and the nested session hang off `RunningProcess.luna`, purely for its `Drop` impl: when the game exits or is killed, the field drops and takes them down in order (LunaTranslator first, then the session — the other way round just makes LunaTranslator crash instead of exit). The session itself stops tint2, picom, openbox, then the X server.

**Don't rebuild the AppImage to test a session change** — it takes ten minutes. `bash desktop/vendor/build.sh` takes about two (only needed after touching `desktop/vendor/`), and config changes need nothing rebuilt but the crate:

```bash
cargo run -p nested_session --example nested -- xmessage hello   # or any X client; no args = idle
cargo test -p nested_session
```

`tests/session.rs` covers: the fullscreen round trip, the EWMH atoms games and Qt rely on, a fixed-size window not being stretched, minimise → taskbar click → restored, the work area leaving room for the taskbar, a compositor owning `_NET_WM_CM_S0`, a 32-bit client keeping 32-bit ancestors (alpha), an always-on-top window plus the taskbar staying above a *focused* fullscreen window, and a game leaving fullscreen getting the windowed size it asked for in either message order (the openbox patches). It skips itself (rather than failing) when there's no Xwayland/Xephyr or the tools aren't built, so it's safe in CI. **Both the example and the tests put a fullscreen window on the host's screen** — ask before running them on someone's machine, or run them where nothing is visible: inside a headless Weston, which the nested Xwayland then attaches to instead of the real session.

```bash
SOCK=drop-headless-$$
env -u DISPLAY weston --backend=headless --socket=$SOCK --width=1280 --height=800 --idle-time=0 & WESTON=$!
env -u DISPLAY -u GAMESCOPE_WAYLAND_DISPLAY WAYLAND_DISPLAY=$SOCK cargo test -p nested_session
# screenshots of a session running there: import -display :<n> -window root shot.png
kill $WESTON
```

That's good for the WM/compositor/taskbar and plain X clients, but not for games: headless Weston has no GPU output, and a Proton game's X connection dies there (`XIO: fatal IO error 110`) before it ever shows a window. Real-game checks need a real display.

**Tear down by PID, never by name.** `pkill -x Xwayland` matches the *host* session's Xwayland and will take the user's whole desktop down with it (this happened). `pkill openbox`/`picom` would likewise hit a host session running them. `pkill -f "[x]message"` is no safer: the bracket trick stops the pattern matching itself, but the word still appears elsewhere in your own command line, so it kills your shell. Record `$!` for what you spawn and kill those pids; everything in the session carries `PR_SET_PDEATHSIG`, so killing `nested` is enough.

LunaTranslator is spawned through `sanitize_external_command` like every other external process, plus its own process group — an AppImage's runtime *forks* its payload instead of exec'ing it, so killing the pid we spawned would leave LunaTranslator running.

### Window/tray behavior

The main window's close button hides to the system tray by default rather than quitting (`on_window_event` + the `RunEvent::ExitRequested` handler in `src-tauri/src/lib.rs`, both gated through the `run_on_tray` helper). This is controlled by the `quit_on_close` setting — both the close-requested and the subsequent exit-requested handlers need to agree, or you get a window that closes but leaves a zombie tray process (that was a real bug — see git history).

### AppImage gotchas (Linux)

The AppImage build has bitten several real bugs, all from the same root cause: linuxdeploy's `AppRun` sets `LD_LIBRARY_PATH`, `PATH`, `PYTHONHOME`, and `PYTHONPATH` to point at the bundle's own libs, and these leak into *every* subprocess the app spawns unless explicitly stripped:
- `open_process_logs` (`src-tauri/src/process.rs`) bypasses the bundled (older) `xdg-open` by calling `/usr/bin/xdg-open` directly with `LD_LIBRARY_PATH` removed — the bundled xdg-utils version is old enough to not know Plasma 6's `kde-open` naming, and the polluted `LD_LIBRARY_PATH` can also break whatever native file-manager helper it execs.
- Game/launcher processes (`process::process_manager::launch_process`) explicitly `env_remove` `PYTHONHOME`, `PYTHONPATH`, and `LD_LIBRARY_PATH` before spawning — without this, Python-based launchers like `umu-run` crash with `Fatal Python error: Failed to import encodings module` because they inherit a `PYTHONHOME` pointing into the (Python-less) AppImage bundle.
- `Dockerfile.build` pins the build's `libwebkit2gtk-4.1`/`libjavascriptcoregtk-4.1` to 2.44.2 (via Ubuntu 23.10/mantic, since it's EOL apt is redirected to `old-releases.ubuntu.com`) — every WebKitGTK release past 2.44 has an unresolved upstream EGL/Wayland regression that aborts the WebProcess on launch, and versions before that lack API symbols `wry`/Tauri need to link.

If you add new code that spawns an external process on Linux, assume it will inherit this pollution and strip what's irrelevant to that process, the same way the two examples above do.
