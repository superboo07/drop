# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project overview

Drop Desktop Client — the desktop app for [Drop](https://github.com/Drop-OSS/drop), a self-hosted game distribution platform. It's a Tauri 2 app: Rust backend, Nuxt 3 + TailwindCSS frontend (chosen specifically so UI components can be shared with Drop's separate web client).

## Setup

This app lives at `desktop/` inside the Drop monorepo. The shared Nuxt UI layer/components it used to pull in as the `libs/drop-base` submodule is now a sibling directory in the same repo, `libraries/base`, which `main/nuxt.config.ts` extends via a relative path (`../../libraries/base`) — so anything that builds this app needs the whole repo present, not just `desktop/`. There are no submodules any more.

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
  - `nested_session` — a nested X server plus a small floating window manager, for running a game and LunaTranslator as draggable windows on one surface (Linux only; see below)
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

**The nested session** exists for the Steam Deck's game mode: gamescope composites exactly one window, so a game and a translator beside it are two things you can only ever look at one at a time. `nested_session` starts a rootful, fullscreen `Xwayland` (or `Xephyr` on an X11 desktop) — which the host compositor sees as one ordinary window, i.e. exactly the one window gamescope will show — and runs a floating reparenting WM inside it, so both are windows the user drags around. It is deliberately **floating, not tiling**: on a controller/trackpad, windows the user positions beat any layout the WM picks, and the chrome is oversized (34px titlebar, 16px bottom grab bar, 40px taskbar) for the same reason.

It is a small WM but not a naive one, because the two things it hosts are the least forgiving X clients there are. What it has to implement, and why:

- **Synthetic `ConfigureNotify` on every move as well as every resize** (`notify_geometry`). A reparented client only sees real ConfigureNotifys whose x/y are relative to its frame, so without this it believes it sits at the chrome offset forever. Wine takes that rect and hands it to the game, so a missing one shows up as *the game rendering at the wrong size in the wrong place* — nothing that looks like a WM bug at all. This shipped broken once.
- **Real `_NET_WM_STATE_FULLSCREEN`.** Omitting it does not keep a game windowed — the game goes fullscreen anyway by resizing itself, and the only effect is that the WM doesn't know, so it ends up with a frame bigger than the root and its client area shoved under a titlebar. Also shipped broken once. A fullscreen window now gets a borderless frame covering the root exactly.
- **Four stacking layers** (`Layer`, `restack`) — desktop / below / normal / above, with the taskbar above all of them. Fullscreen sinks, `_NET_WM_STATE_ABOVE` floats. That, not refusing fullscreen, is what keeps LunaTranslator readable over a fullscreen game.
- **Minimise, and a taskbar to undo it with.** A fullscreen Windows game minimises *itself* on every loss of activation — that's `WM_ACTIVATEAPP` under Wine, not a bug — so dropping the request leaves the game believing it is iconified while still on screen, rendering as a stale strip in a corner. That was the "I tab back in and it's fucked" bug. But honouring it is only half the feature: with no taskbar, a minimised window could only be recovered with Super+Tab, and a Deck in game mode has no keyboard, so minimise silently *lost windows*. `create_taskbar`/`draw_taskbar`/`taskbar_click` own a strip along the bottom edge — the WM's own override-redirect window, absent from `clients`, `order`, `_NET_CLIENT_LIST` and Super+Tab. One entry per managed window **including iconified ones** (dimmed, with a bright bar along the bottom), click to focus or restore. Entries are laid out from `taskbar_order` (insertion order), deliberately not `order` (stacking order), so they don't reshuffle under the cursor on every click. It repaints on manage, unmanage, title change, iconify/restore, focus change and root resize.
- **`_NET_WORKAREA` reserves the taskbar's height** (`work_area`), so maximized windows and new placements stop above it. Fullscreen deliberately ignores it and takes the whole root — the game gets the screen, the strip just stacks on top.
- **`WM_NORMAL_HINTS`** (`Client::constrain`): min/max/base/increments/aspect. Games routinely pin min == max to declare a fixed resolution, and honouring it is the difference between a correctly-sized game and a smeared one. It also decides whether a window is resizable at all.
- **`_NET_WM_WINDOW_TYPE`.** Qt does *not* always set override-redirect on menus, combo popups, tooltips and drag pixmaps — they arrive as ordinary managed windows, and framing one puts a titlebar on a tooltip. Checking override-redirect alone is not enough.
- **`_MOTIF_WM_HINTS`** for the borderless windows Wine and Qt both ask for.
- **The ICCCM focus models** — `WM_TAKE_FOCUS` plus `WM_HINTS.input`. `SetInputFocus` at a client that asked for `WM_TAKE_FOCUS` gives you a window that looks focused and ignores the keyboard, which is Qt's failure mode exactly. `WM_TAKE_FOCUS` needs a real timestamp, hence `last_time`.
- **`_NET_FRAME_EXTENTS`, `_NET_WORKAREA` and the desktop properties.** Qt reads these to place its own dialogs; a toolkit that finds none of them guesses, and guesses badly.
- **`_NET_WM_MOVERESIZE`** so client-side-decorated windows can be dragged — their titlebar is inside the client area where we never see the press.
- `WM_TRANSIENT_FOR` (dialogs centre on their parent and are raised with it), maximize, `WM_CHANGE_STATE`/`_NET_WM_STATE_HIDDEN` for iconification, `_NET_WM_ALLOWED_ACTIONS`, and `WM_STATE` set to Iconic/Withdrawn as ICCCM requires.
- **Titlebar buttons come from one function** (`titlebar_buttons`), used by both `draw_frame` and the hit test in `button_press`. Right to left: close, maximize (omitted for a fixed-size window, with minimise sliding right to fill the gap), minimise. They were two separate pieces of arithmetic once, and drifted into a minimise button that was documented, undrawn and unclickable.
- Super+Tab raises the next window, as a way out of a game that holds a pointer grab and swallows titlebar clicks.
- Both LunaTranslator and the nested server hang off `RunningProcess.luna`, purely for its `Drop` impl: when the game exits or is killed, the field drops and takes them down in order (LunaTranslator first, then the X server — the other way round just makes LunaTranslator crash instead of exit).

**Don't rebuild the AppImage to test a WM change** — it takes ten minutes and the WM is the part most likely to need iterating on. There are two host-side tools:

```bash
cargo run -p nested_session --example nested -- xmessage hello   # or any X client; no args = idle, attach your own
cargo test -p nested_session                                      # drives a real client through the transitions

# `poke` drives the WM against a running session without a human clicking:
cargo run -p nested_session --example poke -- :70 list
cargo run -p nested_session --example poke -- :70 tree        # frame vs client geometry, for desync hunting
cargo run -p nested_session --example poke -- :70 activate 0
cargo run -p nested_session --example poke -- :70 minimise 0  # the WM_CHANGE_STATE Wine sends itself
cargo run -p nested_session --example poke -- :70 restore 0   # clicks that window's taskbar entry
```

`tests/fullscreen.rs` asserts the geometry a client actually ends up with across the windowed → fullscreen → windowed round trip, that a fixed-size window isn't stretched, that a minimised window comes back from a taskbar click, and that the work area leaves room for the strip — i.e. it fails on each of the bugs above. It skips itself (rather than failing) when there's no Xwayland/Xephyr to nest inside, so it's safe in CI.

Two traps when running any of this by hand:

- **Tear down by PID, never by name.** `pkill -x Xwayland` matches the *host* session's Xwayland and will take the user's whole desktop down with it. `pkill -f "[x]message"` is no safer: the bracket trick stops the pattern matching itself, but the word still appears elsewhere in your own command line, so it kills your shell. Record `$!` for what you spawn and kill those pids; the X server carries `PR_SET_PDEATHSIG`, so killing `nested` is usually enough.
- **`poke` syncs before exiting** (`get_input_focus` round trip). `flush()` only pushes bytes at the socket, and a connection closing on their heels can have them dropped — which looks like an action that intermittently does nothing.

Nothing new gets bundled in the AppImage for any of this: `x11rb` is pure Rust (no libX11 to link, and MIT/Apache-2.0 so it doesn't touch Drop's licensing), and an X server has to come from the host to match its graphics stack. LunaTranslator is spawned through `sanitize_external_command` like every other external process, plus its own process group — an AppImage's runtime *forks* its payload instead of exec'ing it, so killing the pid we spawned would leave LunaTranslator running.

### Window/tray behavior

The main window's close button hides to the system tray by default rather than quitting (`on_window_event` + the `RunEvent::ExitRequested` handler in `src-tauri/src/lib.rs`, both gated through the `run_on_tray` helper). This is controlled by the `quit_on_close` setting — both the close-requested and the subsequent exit-requested handlers need to agree, or you get a window that closes but leaves a zombie tray process (that was a real bug — see git history).

### AppImage gotchas (Linux)

The AppImage build has bitten several real bugs, all from the same root cause: linuxdeploy's `AppRun` sets `LD_LIBRARY_PATH`, `PATH`, `PYTHONHOME`, and `PYTHONPATH` to point at the bundle's own libs, and these leak into *every* subprocess the app spawns unless explicitly stripped:
- `open_process_logs` (`src-tauri/src/process.rs`) bypasses the bundled (older) `xdg-open` by calling `/usr/bin/xdg-open` directly with `LD_LIBRARY_PATH` removed — the bundled xdg-utils version is old enough to not know Plasma 6's `kde-open` naming, and the polluted `LD_LIBRARY_PATH` can also break whatever native file-manager helper it execs.
- Game/launcher processes (`process::process_manager::launch_process`) explicitly `env_remove` `PYTHONHOME`, `PYTHONPATH`, and `LD_LIBRARY_PATH` before spawning — without this, Python-based launchers like `umu-run` crash with `Fatal Python error: Failed to import encodings module` because they inherit a `PYTHONHOME` pointing into the (Python-less) AppImage bundle.
- `Dockerfile.build` pins the build's `libwebkit2gtk-4.1`/`libjavascriptcoregtk-4.1` to 2.44.2 (via Ubuntu 23.10/mantic, since it's EOL apt is redirected to `old-releases.ubuntu.com`) — every WebKitGTK release past 2.44 has an unresolved upstream EGL/Wayland regression that aborts the WebProcess on launch, and versions before that lack API symbols `wry`/Tauri need to link.

If you add new code that spawns an external process on Linux, assume it will inherit this pollution and strip what's irrelevant to that process, the same way the two examples above do.
