#!/usr/bin/env bash
# Build script: Docker → pnpm/tauri → AppImage
#
# Usage:
#   bash build_appimage.sh [amd64] [arm64]     (default: both)
#
# No extra tools needed on the host beyond Docker.
# The script builds the builder image (Dockerfile.build, one stage per
# architecture), then mounts the repo into a container and runs the full
# Tauri build inside it, once per architecture.
# arm64 is cross-compiled on the x86_64 build machine: cargo, gcc and the
# nested-session tools build natively for aarch64 against Ubuntu's arm64
# libraries. Only Tauri's AppImage bundler runs arm64 programs (its arm64
# linuxdeploy), under qemu-user, which this script registers with the
# binfmt_misc of a user namespace of its own - nothing is set up on the host,
# and no privileges beyond the container's are needed.
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
# Output: each built .AppImage is copied to the repo root as
# "Drop Desktop Client_<version>_<amd64|arm64>.AppImage", matching tauri-bundler's own
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

# ── Docker wrapper ─────────────────────────────────────────────────────────────
# When invoked on the host, build the image then re-run this script inside it,
# once per architecture asked for.
if [ -z "${DROP_IN_DOCKER:-}" ]; then
    arches=("$@")
    [ "${#arches[@]}" -gt 0 ] || arches=(amd64 arm64)
    for arch in "${arches[@]}"; do
        case "$arch" in
            amd64|arm64) ;;
            *) echo "error: unknown architecture '$arch' (amd64 or arm64)" >&2; exit 2 ;;
        esac
    done

    for arch in "${arches[@]}"; do
        BUILDER_IMAGE="drop-app-builder-$arch"

        # The nested session's openbox/picom/tint2 are built by their own script
        # (and their own, much smaller, builder image) first, so they can be
        # iterated on without a full AppImage build. The container below only
        # copies the result in - see appimage-output-plugin.sh.
        echo ">>> Building nested-session tools ($arch)..."
        bash "$SCRIPT_DIR/vendor/build.sh" "$arch"

        echo ">>> Building Docker builder image ($arch)..."
        docker build -f Dockerfile.build --target "$arch" -t "$BUILDER_IMAGE" .

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
        echo ">>> Running $arch build inside Docker..."
        docker run --rm \
            -e DROP_IN_DOCKER=1 \
            -e DROP_TARGET_ARCH="$arch" \
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
    done
    exit 0
fi

# ── Everything below runs inside the container ────────────────────────────────

TARGET_ARCH="${DROP_TARGET_ARCH:-amd64}"
case "$TARGET_ARCH" in
    amd64)
        APPIMAGE_ARCH=x86_64
        # Native: no --target, so the build stays in target/release.
        RUST_TARGET=""
        NESTED_SESSION_DIR=nested-session
        ;;
    arm64)
        APPIMAGE_ARCH=aarch64
        RUST_TARGET=aarch64-unknown-linux-gnu
        NESTED_SESSION_DIR=nested-session-arm64
        ;;
    *) echo "error: unknown architecture '$TARGET_ARCH'" >&2; exit 2 ;;
esac

if [ "$TARGET_ARCH" = arm64 ]; then
    # Tauri's bundler runs arm64 programs: its arm64 linuxdeploy, and the
    # loader/GTK tools that runs. A user namespace may mount its own binfmt_misc, and
    # what is registered there applies to it alone, so re-run in one and
    # register qemu-user for arm64 ELF files. Root in it is the container's
    # root, so files written to the bind mount are owned as usual.
    if [ -z "${DROP_IN_USERNS:-}" ]; then
        exec unshare --user --map-root-user --mount \
            env DROP_IN_USERNS=1 bash "$SCRIPT_DIR/$(basename "${BASH_SOURCE[0]}")"
    fi
    mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc
    # qemu's own magic/mask for aarch64 (qemu-binfmt-conf.sh). The \x escapes
    # are for the kernel to decode. F: qemu is opened now, not per exec.
    printf '%s' ':qemu-aarch64:M::\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\xb7\x00:\xff\xff\xff\xff\xff\xff\xff\x00\xff\xff\xff\xff\xff\xff\xff\xff\xfe\xff\xff\xff:/usr/bin/qemu-aarch64-static:F' \
        > /proc/sys/fs/binfmt_misc/register

    # Cross-compiling: the arm64 gcc links and builds the -sys crates' C, and
    # pkg-config answers from the arm64 .pc files only (and so does the
    # bundler's GTK plugin, which asks it where GTK lives).
    export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
    export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
    export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
    export AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar
    export PKG_CONFIG_ALLOW_CROSS=1
    export PKG_CONFIG_LIBDIR=/usr/lib/aarch64-linux-gnu/pkgconfig:/usr/share/pkgconfig
    # The GTK plugin asks dpkg-architecture for the library directory it
    # copies GTK's modules, typelibs and loaders from; the environment
    # overrides its answer, which would otherwise be the x86_64 one.
    export DEB_HOST_MULTIARCH=aarch64-linux-gnu
    # ldd and the GTK cache tools that answer for arm64 (Dockerfile.build).
    export PATH="/opt/cross-arm64:$PATH"
fi

# This container has no TTY, so pnpm can't prompt to confirm purging/
# reinstalling node_modules when the lockfile/workspace config doesn't match
# what's already there - CI mode answers that non-interactively instead of
# aborting. It's set on the pnpm commands alone (here, and in build.mjs for
# the frontend's install), never exported: cargo reads CI too, and turns
# incremental compilation off when it's set, overriding [profile.release].

# The AppImage runtime is baked into the builder image (Dockerfile.build);
# appimage-output-plugin.sh (step 3) hands it to appimagetool, which would
# otherwise download it on every build.
export LDAI_RUNTIME_FILE="/opt/appimage-runtime-$APPIMAGE_ARCH"

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
echo ">>> Building version $APP_VERSION for $TARGET_ARCH"

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
#
# The bundler has linuxdeploy prepare the AppDir, then pack it with the
# output plugin in its tools directory - downloaded only when missing, so
# ours is put there under that name, every build. It finishes the AppDir
# (libwayland out; umu-run, winetricks and the nested-session tools in) and
# packs it: the AppImage is compressed once, and comes out of the bundler
# finished. (The file name carries no architecture, and the tools volume is
# shared by both builds, so a downloaded one would be whichever architecture
# happened to build first.)
TAURI_TOOLS=/root/.cache/tauri
mkdir -p "$TAURI_TOOLS"
install -m 0755 "/workspace/$APP_DIR/appimage-output-plugin.sh" \
    "$TAURI_TOOLS/linuxdeploy-plugin-appimage.AppImage"
export DROP_NESTED_SESSION_TOOLS="/workspace/$APP_DIR/vendor/out/$NESTED_SESSION_DIR"

echo ">>> Running tauri build (appimage only)..."
# CARGO_TERM_VERBOSE=true (passed in from the host) makes this verbose too,
# which is how to see cargo's "Dirty <crate>: <reason>" for a rebuild -
# tauri drops cargo's own verbose lines unless it's verbose itself.
pnpm tauri build ${CARGO_TERM_VERBOSE:+--verbose} ${RUST_TARGET:+--target "$RUST_TARGET"} \
    --bundles appimage --config "{\"version\": \"$APP_VERSION\"}"
# Where cargo put this build: target/release natively, target/<triple>/release
# when cross-compiling.
RELEASE_DIR="$PWD/src-tauri/target/${RUST_TARGET:+$RUST_TARGET/}release"

# tauri-codegen caches what it embeds - the brotli'd frontend
# (tauri-codegen-assets/) and the app icon - as files in drop-app's OUT_DIR,
# named by their content's checksum and written only when missing, then
# include_bytes!s them. New ones are created *during* the compile, so they're
# newer than cargo's record of when that compile started, and the next build
# recompiles drop-app over them - once more after every frontend or icon
# change, for nothing. Content-addressed files can't be stale, so backdate
# them all.
find "$RELEASE_DIR"/build/drop-app -type f -regextype posix-extended \
    -regex '.*/out/(tauri-codegen-assets/)?[0-9a-f]{64}(\.[A-Za-z0-9]+)?' \
    -exec touch -m -d @1 {} +

# ── 4. Find the AppImage ──────────────────────────────────────────────────────
# tauri-bundler names its output "{productName}_{version}_{arch}.AppImage", so
# the name has spaces in it and changes whenever the version in
# tauri.conf.json does - hence globbing for it rather than spelling it out.
# Insist on exactly one match: leftovers from an older version would otherwise
# be picked up (or silently concatenated) and packaged as this build's output.
BUNDLE_DIR="$RELEASE_DIR/bundle/appimage"
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

# ── 5. Copy the result out to the repo root ───────────────────────────────────
OUTPUT_NAME="Drop Desktop Client_${APP_VERSION}_${TARGET_ARCH}.AppImage"
# The repo root, not desktop/, so built AppImages all collect in one place.
# Via a temp name and a rename, since the previous build's AppImage is quite
# possibly running right now - cp can't overwrite an executing file ("Text
# file busy"); a rename just replaces the directory entry.
cp "$APPIMAGE" "$REPO_ROOT/.$OUTPUT_NAME.tmp"
mv -f "$REPO_ROOT/.$OUTPUT_NAME.tmp" "$REPO_ROOT/$OUTPUT_NAME"

echo ""
echo "Done: $OUTPUT_NAME"
