#!/usr/bin/env bash
# Build script: Docker → pnpm/tauri → AppImage
#
# Usage:
#   bash build_appimage.sh
#
# No extra tools needed on the host beyond Docker.
# The script builds the builder image (Dockerfile.build) once, then mounts
# the repo into a container and runs the full Tauri build inside it.
# Output: the built .AppImage is copied to the repo root.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

BUILDER_IMAGE="drop-app-builder"

# ── Docker wrapper ─────────────────────────────────────────────────────────────
# When invoked on the host, build the image then re-run this script inside it.
if [ -z "${DROP_IN_DOCKER:-}" ]; then
    echo ">>> Building Docker builder image..."
    docker build -f Dockerfile.build -t "$BUILDER_IMAGE" .

    echo ">>> Running build inside Docker..."
    docker run --rm \
        -e DROP_IN_DOCKER=1 \
        -v "$SCRIPT_DIR":/workspace \
        -w /workspace \
        "$BUILDER_IMAGE" \
        bash build_appimage.sh
    exit $?
fi

# ── Everything below runs inside the container ────────────────────────────────

# ── 1. Make sure submodules (libs/drop-base, tailscale) are present ───────────
echo ">>> Fetching submodules..."
git config --global --add safe.directory /workspace
git submodule update --init --recursive

# ── 2. Install root deps (tauri CLI) ──────────────────────────────────────────
echo ">>> Installing dependencies..."
pnpm install

# ── 3. Build the frontend(s) + Tauri AppImage bundle ──────────────────────────
# beforeBuildCommand ("pnpm build") builds the Nuxt view into ./.output,
# then tauri-bundler packages everything into an AppImage.
echo ">>> Running tauri build (appimage only)..."
pnpm tauri build --bundles appimage

# ── 4. Inject vendored umu-run/winetricks (Steam Deck etc. support) ───────────
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

# ── 5. Copy the result out to the repo root ───────────────────────────────────
cp "$APPIMAGE" ./

echo ""
echo "Done: $(basename "$APPIMAGE")"
