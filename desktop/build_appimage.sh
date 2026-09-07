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
# Output: the built .AppImage is copied to the repo root, named after the
# commit it was built from (Drop Desktop Client_<short-sha>_amd64.AppImage,
# matching tauri-bundler's own "{productName}_{version}_{arch}" convention
# but with the commit hash standing in for the version) rather than the
# tauri-bundler default (which is version-tag based) -- this is the single
# place that naming scheme is decided, so anything invoking this script
# (CI or otherwise) doesn't need to duplicate the logic.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# Path of desktop/ relative to the repo root, so the same value works on the
# host and at /workspace inside the container.
APP_DIR="${SCRIPT_DIR#"$REPO_ROOT"/}"
cd "$SCRIPT_DIR"

BUILDER_IMAGE="drop-app-builder"

# ── Fast mode (DROP_FAST=1) ───────────────────────────────────────────────────
# Release builds here use `lto = true` + `codegen-units = 1` (src-tauri's
# [profile.release]), which is most of a clean build's wall time and buys
# nothing when you just want to click around in the app. DROP_FAST=1 overrides
# those through cargo's env-var profile overrides, so the checked-in profile -
# and therefore what CI and releases produce - is untouched.
#
# The overrides change every crate's fingerprint, so a fast build and a real
# build would otherwise invalidate each other's artifacts on every switch.
# Fast mode gets its own target directory (a named volume) to keep the two
# incremental caches side by side.

# ── Docker wrapper ─────────────────────────────────────────────────────────────
# When invoked on the host, build the image then re-run this script inside it.
if [ -z "${DROP_IN_DOCKER:-}" ]; then
    if [ -n "${DROP_FAST:-}" ]; then
        echo ">>> DROP_FAST set: thin LTO, parallel codegen, separate target dir."
        echo ">>> Dev builds only - use a normal build for anything you ship."
    fi

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
    echo ">>> Running build inside Docker..."
    docker run --rm \
        -e DROP_IN_DOCKER=1 \
        -e DROP_FAST \
        -e npm_config_store_dir=/pnpm-store \
        -v "$REPO_ROOT":/workspace \
        -v drop-appimage-pnpm-store:/pnpm-store \
        -v drop-appimage-node-modules:/workspace/node_modules \
        -v drop-appimage-main-node-modules:"/workspace/$APP_DIR/main/node_modules" \
        -v drop-appimage-cargo-registry:/root/.cargo/registry \
        -v drop-appimage-cargo-git:/root/.cargo/git \
        -v drop-appimage-target-fast:/cargo-target-fast \
        -w /workspace \
        "$BUILDER_IMAGE" \
        bash "$APP_DIR/build_appimage.sh"
    exit $?
fi

# ── Everything below runs inside the container ────────────────────────────────

# This container has no TTY, so pnpm can't prompt to confirm purging/
# reinstalling node_modules when the lockfile/workspace config doesn't match
# what's already there - CI mode answers that non-interactively instead of
# aborting. Exported (not inlined per-command) since build.mjs's own `pnpm
# install` inside main/ (triggered by `pnpm tauri build`'s beforeBuildCommand
# below) needs it too, not just the install on the next line.
export CI=true

# See the DROP_FAST notes above. These are cargo's documented env overrides for
# [profile.release] keys, so nothing in Cargo.toml changes - `panic = "abort"`
# and the rest of the profile still apply, and an unset DROP_FAST builds
# exactly what it always did.
if [ -n "${DROP_FAST:-}" ]; then
    export CARGO_PROFILE_RELEASE_LTO=thin
    export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
    export CARGO_PROFILE_RELEASE_INCREMENTAL=true
    export CARGO_TARGET_DIR=/cargo-target-fast
fi

# ── 1. Install desktop deps (tauri CLI) from the workspace root ─────────────
# desktop/ is a member of the monorepo's pnpm workspace, so the install has to
# run from the root. --filter keeps it to this package rather than also
# installing server/ and sites/, which the AppImage build doesn't need. (The
# Nuxt view under main/ has its own lockfile and is installed separately by
# build.mjs, via tauri's beforeBuildCommand below.) There are no submodules to
# fetch any more -- the monorepo vendors what used to be libs/drop-base.
echo ">>> Installing dependencies..."
git config --global --add safe.directory /workspace
cd /workspace
pnpm install --filter drop-app
cd "/workspace/$APP_DIR"

# ── 2. Build the frontend(s) + Tauri AppImage bundle ──────────────────────────
# beforeBuildCommand ("pnpm build") builds the Nuxt view into ./.output,
# then tauri-bundler packages everything into an AppImage.
echo ">>> Running tauri build (appimage only)..."
pnpm tauri build --bundles appimage

# ── 3. Inject vendored umu-run/winetricks (Steam Deck etc. support) ───────────
# These have no distro package manager to install umu-launcher/winetricks on,
# so we bundle known-working copies as a fallback. Placed in their own
# directory (not usr/bin) so they don't get caught up in the sanitize step
# that strips the AppImage's usr/bin from PATH before spawning external
# tools (see utils::external_open::sanitize_external_command) -- that step
# re-adds this specific directory back.
# Honour CARGO_TARGET_DIR (fast mode points it at a volume); tauri-bundler
# writes its bundles under whichever target directory cargo used.
TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/src-tauri/target}"
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
mkdir -p "$APPDIR/usr/libexec/drop-tools"
cp /opt/drop-vendor/umu-run /opt/drop-vendor/winetricks "$APPDIR/usr/libexec/drop-tools/"

echo ">>> Repacking AppImage..."
rm -f "$APPIMAGE"
ARCH=x86_64 appimagetool "$APPDIR" "$APPIMAGE"

# ── 4. Copy the result out to the repo root, named after the commit ───────────
SHORT_SHA=$(git rev-parse --short HEAD)
# Fast builds are tagged so they can't be confused with a shippable artifact
# built from the same commit. CI never sets DROP_FAST, so its glob is unchanged.
OUTPUT_NAME="Drop Desktop Client_${SHORT_SHA}${DROP_FAST:+-fast}_amd64.AppImage"
# The repo root, not desktop/ -- that's where CI globs for the artifact.
cp "$APPIMAGE" "$REPO_ROOT/$OUTPUT_NAME"

echo ""
echo "Done: $OUTPUT_NAME"
