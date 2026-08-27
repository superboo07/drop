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

# ── Docker wrapper ─────────────────────────────────────────────────────────────
# When invoked on the host, build the image then re-run this script inside it.
if [ -z "${DROP_IN_DOCKER:-}" ]; then
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
    echo ">>> Running build inside Docker..."
    docker run --rm \
        -e DROP_IN_DOCKER=1 \
        -e npm_config_store_dir=/pnpm-store \
        -v "$REPO_ROOT":/workspace \
        -v drop-appimage-pnpm-store:/pnpm-store \
        -v drop-appimage-node-modules:/workspace/node_modules \
        -v drop-appimage-main-node-modules:"/workspace/$APP_DIR/main/node_modules" \
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
APPIMAGE=$(ls src-tauri/target/release/bundle/appimage/*.AppImage)
echo ">>> Injecting vendored umu-run/winetricks..."
rm -rf squashfs-root
"$APPIMAGE" --appimage-extract >/dev/null
mkdir -p squashfs-root/usr/libexec/drop-tools
cp /opt/drop-vendor/umu-run /opt/drop-vendor/winetricks squashfs-root/usr/libexec/drop-tools/

echo ">>> Repacking AppImage..."
rm -f "$APPIMAGE"
ARCH=x86_64 appimagetool squashfs-root "$APPIMAGE"
rm -rf squashfs-root

# ── 4. Copy the result out to the repo root, named after the commit ───────────
SHORT_SHA=$(git rev-parse --short HEAD)
OUTPUT_NAME="Drop Desktop Client_${SHORT_SHA}_amd64.AppImage"
# The repo root, not desktop/ -- that's where CI globs for the artifact.
cp "$APPIMAGE" "$REPO_ROOT/$OUTPUT_NAME"

echo ""
echo "Done: $OUTPUT_NAME"
