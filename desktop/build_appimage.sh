#!/usr/bin/env bash
# Build script: Docker → pnpm/tauri → AppImage
#
# Usage:
#   bash build_appimage.sh
#
# No extra tools needed on the host beyond Docker.
# The script builds the builder image (Dockerfile.build) once, then mounts
# the repo into a container and runs the full Tauri build inside it.
# The whole monorepo (not just desktop/) is mounted, since the pnpm
# workspace root and the .git directory both live one level up and the
# build needs each of them - the workspace to resolve desktop/'s deps, and
# .git for the commit hash the output is named after.
# Version: tauri.conf.json's version is a fixed release number, so on its own
# every build claims to be that version whatever commit it came from. This
# script stamps the commit onto it instead - <conf version>-g<short sha>, plus
# ".dirty" when the tree had uncommitted changes to tracked files - and passes
# that to tauri via --config, so the AppImage says exactly what it was built
# from. tauri.conf.json itself is untouched, so other platforms' builds (which
# have stricter version formats) are unaffected.
# Output: the built .AppImage is copied to the repo root as
# "Drop Desktop Client_<version>_amd64.AppImage", matching tauri-bundler's own
# "{productName}_{version}_{arch}" convention. This is the single place that
# naming scheme is decided, so anything invoking this script doesn't need to
# duplicate the logic.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# Path of desktop/ relative to the repo root, so the same value works on the
# host and at /workspace inside the container.
APP_DIR="${SCRIPT_DIR#"$REPO_ROOT"/}"
cd "$SCRIPT_DIR"

BUILDER_IMAGE="drop-app-builder"

# ── Docker wrapper ─────────────────────────────────────────────────────────────
# When invoked on the host, build the image then re-run this script inside it.
if [ -z "${DROP_IN_DOCKER:-}" ]; then
    # The nested session's openbox/picom/tint2 are built by their own script
    # (and their own, much smaller, builder image) first, so they can be
    # iterated on without a full AppImage build. The container below only
    # copies the result in - see step 4.
    echo ">>> Building nested-session tools..."
    bash "$SCRIPT_DIR/vendor/build.sh"

    echo ">>> Building Docker builder image..."
    docker build -f Dockerfile.build -t "$BUILDER_IMAGE" .

    # Keep node_modules and the pnpm store on named volumes rather than on the
    # bind mount. pnpm places its store next to the project so it can hardlink,
    # which inside the container means /workspace/.pnpm-store -- a path that can
    # never match the host's store. On a mismatch pnpm reconfigures and purges
    # node_modules before reinstalling, so sharing those directories means every
    # AppImage build wipes whatever the host had installed (leaving, say, the
    # server workspace with no dependencies) and drops a .pnpm-store inside the
    # repo. Volumes also let the deps survive between builds.
    # The cargo registry lives in the container's own filesystem, which --rm
    # throws away, so without a volume here every build re-downloads and
    # re-unpacks the whole dependency tree before it can even start compiling.
    # (The build directory itself doesn't need one - it's src-tauri/target on
    # the bind mount, so it already persists on the host.)
    # Tauri's bundler likewise downloads linuxdeploy, its plugins and AppRun
    # into ~/.cache/tauri on first use, and corepack the exact pnpm the repo
    # pins into its own cache - every build, without volumes.
    echo ">>> Running build inside Docker..."
    docker run --rm \
        -e DROP_IN_DOCKER=1 \
        -e CARGO_TERM_VERBOSE \
        -e pnpm_config_store_dir=/pnpm-store \
        -v "$REPO_ROOT":/workspace \
        -v drop-appimage-pnpm-store:/pnpm-store \
        -v drop-appimage-node-modules:/workspace/node_modules \
        -v drop-appimage-main-node-modules:"/workspace/$APP_DIR/main/node_modules" \
        -v drop-appimage-cargo-registry:/root/.cargo/registry \
        -v drop-appimage-cargo-git:/root/.cargo/git \
        -v drop-appimage-tauri-tools:/root/.cache/tauri \
        -v drop-appimage-corepack:/root/.cache/node/corepack \
        -w /workspace \
        "$BUILDER_IMAGE" \
        bash "$APP_DIR/build_appimage.sh"
    exit $?
fi

# ── Everything below runs inside the container ────────────────────────────────

# This container has no TTY, so pnpm can't prompt to confirm purging/
# reinstalling node_modules when the lockfile/workspace config doesn't match
# what's already there - CI mode answers that non-interactively instead of
# aborting. It's set on the pnpm commands alone (here, and in build.mjs for
# the frontend's install), never exported: cargo reads CI too, and turns
# incremental compilation off when it's set, overriding [profile.release].

# The AppImage runtime is baked into the builder image (Dockerfile.build).
# tauri's bundler packs its AppImage through linuxdeploy's appimage plugin,
# which downloads the runtime on every build unless told where one is; the
# repack in step 4 passes it to appimagetool directly.
export LDAI_RUNTIME_FILE=/opt/appimage-runtime-x86_64

# ── 1. Work out the version to stamp into this build ──────────────────────────
# The base comes from tauri.conf.json (the release number the branch is
# working towards); the commit is appended as a semver prerelease, which keeps
# it a valid version for tauri while making every build self-identifying.
#
# Dirtiness counts tracked changes only. The repo root accumulates untracked
# build output - previous AppImages, drop-custom.tar.gz - so including
# untracked files would mark essentially every build dirty and tell you
# nothing.
git config --global --add safe.directory /workspace
BASE_VERSION="$(node -p "require('/workspace/$APP_DIR/src-tauri/tauri.conf.json').version")"
GIT_SHA="$(git -C /workspace rev-parse --short HEAD)"
if git -C /workspace diff --quiet HEAD --; then
    GIT_DIRTY=""
else
    GIT_DIRTY=".dirty"
fi
# The "g" prefix is git describe's convention, and it also keeps the
# identifier from ever being all-digits, which semver would reject.
APP_VERSION="${BASE_VERSION}-g${GIT_SHA}${GIT_DIRTY}"
echo ">>> Building version $APP_VERSION"

# ── 2. Install desktop deps (tauri CLI) from the workspace root ─────────────
# desktop/ is a member of the monorepo's pnpm workspace, so the install has to
# run from the root. --filter keeps it to this package rather than also
# installing server/ and sites/, which the AppImage build doesn't need. (The
# Nuxt view under main/ has its own lockfile and is installed separately by
# build.mjs, via tauri's beforeBuildCommand below.) There are no submodules to
# fetch here -- the monorepo vendors what used to be libs/drop-base, and the
# only submodules left (desktop/vendor) were built before the container
# started.
echo ">>> Installing dependencies..."
cd /workspace
CI=true pnpm install --filter drop-app

# Stop on high/critical advisories in what the AppImage ships: this package's
# production deps and the Nuxt view's (its own lockfile). Same gate as the
# release workflow's audit job. Accept one that doesn't apply with
# `auditConfig.ignoreGhsas` in the relevant pnpm-workspace.yaml.
echo ">>> Auditing dependencies..."
pnpm --filter drop-app... audit --prod --audit-level high
pnpm --dir "$APP_DIR/main" audit --prod --audit-level high
cd "/workspace/$APP_DIR"

# ── 3. Build the frontend(s) + Tauri AppImage bundle ──────────────────────────
# beforeBuildCommand ("pnpm build") builds the Nuxt view into ./.output,
# then tauri-bundler packages everything into an AppImage. --config overrides
# just the version for this build; it's merged over tauri.conf.json rather
# than editing it, so nothing has to be reverted afterwards.
echo ">>> Running tauri build (appimage only)..."
# CARGO_TERM_VERBOSE=true (passed in from the host) makes this verbose too,
# which is how to see cargo's "Dirty <crate>: <reason>" for a rebuild -
# tauri drops cargo's own verbose lines unless it's verbose itself.
pnpm tauri build ${CARGO_TERM_VERBOSE:+--verbose} --bundles appimage --config "{\"version\": \"$APP_VERSION\"}"

# tauri-codegen caches what it embeds - the brotli'd frontend
# (tauri-codegen-assets/) and the app icon - as files in drop-app's OUT_DIR,
# named by their content's checksum and written only when missing, then
# include_bytes!s them. New ones are created *during* the compile, so they're
# newer than cargo's record of when that compile started, and the next build
# recompiles drop-app over them - once more after every frontend or icon
# change, for nothing. Content-addressed files can't be stale, so backdate
# them all.
find src-tauri/target/release/build/drop-app -type f -regextype posix-extended \
    -regex '.*/out/(tauri-codegen-assets/)?[0-9a-f]{64}(\.[A-Za-z0-9]+)?' \
    -exec touch -m -d @1 {} +

# ── 4. Inject vendored umu-run/winetricks (Steam Deck etc. support) ───────────
# These have no distro package manager to install umu-launcher/winetricks on,
# so we bundle known-working copies as a fallback. Placed in their own
# directory (not usr/bin) so they don't get caught up in the sanitize step
# that strips the AppImage's usr/bin from PATH before spawning external
# tools (see utils::external_open::sanitize_external_command) -- that step
# re-adds this specific directory back.
TARGET_DIR="$PWD/src-tauri/target"
BUNDLE_DIR="$TARGET_DIR/release/bundle/appimage"

# tauri-bundler names its output "{productName}_{version}_{arch}.AppImage", so
# the name has spaces in it and changes whenever the version in
# tauri.conf.json does - hence globbing for it rather than spelling it out.
# Insist on exactly one match: leftovers from an older version would otherwise
# be picked up (or silently concatenated) and packaged as this build's output.
shopt -s nullglob
appimages=("$BUNDLE_DIR"/*.AppImage)
shopt -u nullglob
if [ "${#appimages[@]}" -ne 1 ]; then
    echo "error: expected exactly one .AppImage in $BUNDLE_DIR, found ${#appimages[@]}" >&2
    [ "${#appimages[@]}" -eq 0 ] || printf '  %s\n' "${appimages[@]}" >&2
    echo "delete the stale ones and re-run, or wipe the bundle directory." >&2
    exit 1
fi
APPIMAGE="${appimages[0]}"

# Unpack next to the bundle rather than into the repo, so a build that dies
# midway doesn't leave a root-owned squashfs-root/ behind in desktop/ - and
# so the (large) extracted tree lands on the same filesystem it came from.
WORK_DIR="$(mktemp -d "$BUNDLE_DIR/.repack.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT
APPDIR="$WORK_DIR/squashfs-root"

echo ">>> Injecting vendored umu-run/winetricks..."
(cd "$WORK_DIR" && "$APPIMAGE" --appimage-extract >/dev/null)
# linuxdeploy bundles this image's libwayland-*, but Mesa's libEGL is on the
# AppImage excludelist and always comes from the host. A host with a newer Mesa
# then loads its libEGL against our older libwayland-client, and WebKit's
# WebProcess aborts on launch with "Could not create default EGL display:
# EGL_BAD_PARAMETER" - reproduced by putting 24.04's libwayland 1.22 back
# against a host with Mesa 26 / libwayland 1.26: blank window and that abort;
# without it the same build renders. libwayland is part of that same
# host graphics stack - every system that can run GTK 3 has it - so take it
# from the host too. (This crash was once blamed on WebKitGTK itself and
# "fixed" by pinning an old one; don't - see Dockerfile.build.)
echo ">>> Dropping bundled libwayland (must match the host's Mesa)..."
rm -f "$APPDIR"/usr/lib/libwayland-*.so*

mkdir -p "$APPDIR/usr/libexec/drop-tools"
cp /opt/drop-vendor/umu-run /opt/drop-vendor/winetricks "$APPDIR/usr/libexec/drop-tools/"

# The nested session's window manager, compositor and taskbar, as built by
# vendor/build.sh on the host side of this script. A self-contained prefix
# (own lib/, RUNPATH $ORIGIN/../lib), found by nested_session::tools at
# $APPDIR/usr/libexec/drop-tools/nested-session. In a subdirectory, so the
# PATH entry sanitize_external_command adds for drop-tools doesn't expose
# them to games.
echo ">>> Injecting nested-session tools..."
NESTED_TOOLS="/workspace/$APP_DIR/vendor/out/nested-session"
if [ ! -x "$NESTED_TOOLS/bin/openbox" ]; then
    echo "error: $NESTED_TOOLS is missing - vendor/build.sh should have built it" >&2
    exit 1
fi
cp -a "$NESTED_TOOLS" "$APPDIR/usr/libexec/drop-tools/nested-session"

echo ">>> Repacking AppImage..."
rm -f "$APPIMAGE"
# The runtime is baked into the builder image; without --runtime-file,
# appimagetool downloads it on every build.
ARCH=x86_64 appimagetool --runtime-file /opt/appimage-runtime-x86_64 "$APPDIR" "$APPIMAGE"

# ── 5. Copy the result out to the repo root ───────────────────────────────────
OUTPUT_NAME="Drop Desktop Client_${APP_VERSION}_amd64.AppImage"
# The repo root, not desktop/, so built AppImages all collect in one place.
# Via a temp name and a rename, since the previous build's AppImage is quite
# possibly running right now - cp can't overwrite an executing file ("Text
# file busy"); a rename just replaces the directory entry.
cp "$APPIMAGE" "$REPO_ROOT/.$OUTPUT_NAME.tmp"
mv -f "$REPO_ROOT/.$OUTPUT_NAME.tmp" "$REPO_ROOT/$OUTPUT_NAME"

echo ""
echo "Done: $OUTPUT_NAME"
