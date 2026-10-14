#!/usr/bin/env bash
# Builds openbox, picom and tint2 (the submodules in this directory) into a
# self-contained, relocatable prefix for the nested session:
#
#   desktop/vendor/out/nested-session/          (amd64)
#   desktop/vendor/out/nested-session-arm64/    (arm64)
#     bin/   openbox picom tint2       (RPATH $ORIGIN/../lib)
#     lib/   their shared libraries    (RPATH $ORIGIN)
#
# Usage:
#   bash desktop/vendor/build.sh [amd64|arm64]     (default amd64)
#
# arm64 is cross-compiled on the x86_64 build machine, against Ubuntu's arm64
# libraries (see ./Dockerfile); nothing is emulated.
#
# Runs in Docker (see ./Dockerfile) - nothing is built on or installed to the
# host. build_appimage.sh copies the prefix into the AppImage; for testing
# on the host without an AppImage, point DROP_NESTED_SESSION_TOOLS at it.
#
# Everything these three link against is bundled except the libraries that
# have to come from the host to work at all - libc and friends, and the X11/
# font/GL stack, which must match the host's X server and fonts. That's the
# same split AppImage's own excludelist makes.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
VENDOR_DIR="${SCRIPT_DIR#"$REPO_ROOT"/}"
TARGET_ARCH="${1:-${DROP_TARGET_ARCH:-amd64}}"
case "$TARGET_ARCH" in
    amd64) OUT_NAME=nested-session TRIPLE=x86_64-linux-gnu ;;
    arm64) OUT_NAME=nested-session-arm64 TRIPLE=aarch64-linux-gnu ;;
    *) echo "error: unknown architecture '$TARGET_ARCH' (amd64 or arm64)" >&2; exit 2 ;;
esac
OUT_DIR="$SCRIPT_DIR/out/$OUT_NAME"
BUILDER_IMAGE="drop-nested-session-builder-$TARGET_ARCH"

# ── Docker wrapper ─────────────────────────────────────────────────────────────
if [ -z "${DROP_IN_DOCKER:-}" ]; then
    for module in openbox picom tint2 libconfig; do
        if [ ! -e "$SCRIPT_DIR/$module/.git" ]; then
            echo "error: $VENDOR_DIR/$module is empty - run" >&2
            echo "  git submodule update --init $VENDOR_DIR" >&2
            exit 1
        fi
    done

    # Skip the whole thing when nothing that goes into the output has
    # changed since it was last built: each submodule's commit (and any
    # local edits to it), Drop's patches, this script and the builder's
    # Dockerfile. build_appimage.sh calls this on every build, and
    # rebuilding three C projects from scratch each time was pure waste.
    # DROP_VENDOR_REBUILD=1 forces a rebuild anyway.
    inputs_hash="$(
        for module in openbox picom tint2 libconfig; do
            echo "$module $(git -C "$SCRIPT_DIR/$module" rev-parse HEAD)"
            git -C "$SCRIPT_DIR/$module" status --porcelain
        done
        cd "$SCRIPT_DIR" && sha256sum build.sh Dockerfile patches/*
    )"
    inputs_hash="$(printf '%s\n' "$inputs_hash" | sha256sum | cut -d' ' -f1)"
    stamp="$OUT_DIR/.inputs-sha256"
    if [ -z "${DROP_VENDOR_REBUILD:-}" ] \
        && [ -x "$OUT_DIR/bin/openbox" ] \
        && [ "$(cat "$stamp" 2>/dev/null)" = "$inputs_hash" ]; then
        echo ">>> Nested-session tools are up to date ($VENDOR_DIR/out/$OUT_NAME)"
        exit 0
    fi

    echo ">>> Building nested-session builder image ($TARGET_ARCH)..."
    docker build --target "$TARGET_ARCH" -t "$BUILDER_IMAGE" "$SCRIPT_DIR"

    # Keep the output owned by the invoking user. Rootful Docker needs
    # --user for that; rootless Docker already maps the container's root to
    # the invoking user, and there --user would map to an unprivileged
    # subordinate uid that can't write to the mount at all.
    user_args=()
    if ! docker info --format '{{.SecurityOptions}}' 2>/dev/null | grep -q rootless; then
        user_args=(--user "$(id -u):$(id -g)")
    fi

    # The source tree is mounted read-only: every build happens in a copy, so
    # openbox's bootstrap and meson/cmake can't leave generated files behind
    # in the submodules.
    echo ">>> Building openbox, picom and tint2 ($TARGET_ARCH) inside Docker..."
    mkdir -p "$SCRIPT_DIR/out"
    docker run --rm \
        "${user_args[@]}" \
        -e DROP_IN_DOCKER=1 \
        -e DROP_TARGET_ARCH="$TARGET_ARCH" \
        -e HOME=/tmp \
        -v "$REPO_ROOT":/workspace:ro \
        -v "$SCRIPT_DIR/out":/out \
        -w /workspace \
        "$BUILDER_IMAGE" \
        bash "$VENDOR_DIR/build.sh"
    echo "$inputs_hash" > "$stamp"
    echo ""
    echo "Done: $OUT_DIR"
    exit 0
fi

# ── Everything below runs inside the container ────────────────────────────────

SRC=/workspace/$VENDOR_DIR
BUILD=/tmp/build
# Never exists at runtime; nothing here reads its compiled-in prefix (the
# nested session passes every config and theme path explicitly), it only
# has to be somewhere to install to before the tree is made relocatable.
PREFIX=/tmp/prefix
JOBS="$(nproc)"

# Cross-compiling (arm64): the compilers are the $TRIPLE- ones, and
# pkg-config looks only at that architecture's .pc files. Each build system
# is told below in its own way; a native build passes none of it, since
# e.g. autoconf given --host for the machine it runs on still assumes a
# cross build.
CROSS=""
if [ "$TRIPLE" != "$(gcc -dumpmachine)" ]; then
    CROSS=1
    export PKG_CONFIG_LIBDIR="/usr/lib/$TRIPLE/pkgconfig:/usr/share/pkgconfig"
    cat > /tmp/meson-cross.txt <<EOF
[binaries]
c = '$TRIPLE-gcc'
cpp = '$TRIPLE-g++'
ar = '$TRIPLE-ar'
strip = '$TRIPLE-strip'
pkg-config = 'pkg-config'

[properties]
pkg_config_libdir = ['/usr/lib/$TRIPLE/pkgconfig', '/usr/share/pkgconfig']

[host_machine]
system = 'linux'
cpu_family = 'aarch64'
cpu = 'aarch64'
endian = 'little'
EOF
fi

rm -rf "$BUILD" "$PREFIX"
mkdir -p "$BUILD" "$PREFIX"
# -L: the submodules' .git entries are files pointing into the superproject's
# .git/modules, which isn't useful in a copy - drop them rather than copy a
# dangling pointer that would make meson's `git describe` complain.
for module in openbox picom tint2; do
    cp -a "$SRC/$module" "$BUILD/$module"
    rm -f "$BUILD/$module/.git"
done
# The builder image has no libconfig-dev (the vendored one used to be needed
# because Ubuntu 23.10 shipped 1.5 and picom needs >= 1.7; it stays so picom
# keeps linking it statically), so picom falls back to its libconfig
# subproject. Its wrap would git-clone that
# at build time; the libconfig submodule, pinned to the same revision as
# picom's subprojects/libconfig.wrap, is put where meson looks first instead.
cp -a "$SRC/libconfig" "$BUILD/picom/subprojects/libconfig"
rm -f "$BUILD/picom/subprojects/libconfig/.git"

echo ">>> openbox"
(
    cd "$BUILD/openbox"
    # Drop's own changes, applied to the copy so the submodule stays upstream.
    for patch in "$SRC"/patches/openbox-*.patch; do
        patch -p1 --forward < "$patch"
    done
    ./bootstrap >/dev/null
    # A git checkout generates its man pages with docbook-to-man; nothing
    # here needs them, so stand in empty ones make will treat as up to date.
    for page in doc/*.1.sgml; do : > "${page%.sgml}.in"; done
    ./configure --prefix="$PREFIX" \
        ${CROSS:+--host=$TRIPLE} \
        --disable-nls \
        --disable-startup-notification \
        --disable-imlib2 \
        --disable-librsvg \
        --disable-session-management \
        --disable-xkb
    make -j"$JOBS"
    make install
)

echo ">>> picom"
(
    cd "$BUILD/picom"
    meson setup build --prefix="$PREFIX" --buildtype=release --wrap-mode=nodownload \
        ${CROSS:+--cross-file /tmp/meson-cross.txt} \
        -Dopengl=false -Ddbus=false -Dregex=false \
        -Dcompton=false -Dwith_docs=false
    ninja -C build -j"$JOBS"
    ninja -C build install
)

echo ">>> tint2"
(
    cd "$BUILD/tint2"
    cmake_cross=()
    if [ -n "$CROSS" ]; then
        cmake_cross=(-DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=aarch64
            -DCMAKE_C_COMPILER="$TRIPLE-gcc" -DCMAKE_CXX_COMPILER="$TRIPLE-g++")
    fi
    cmake -S . -B build -DCMAKE_INSTALL_PREFIX="$PREFIX" -DCMAKE_BUILD_TYPE=Release \
        "${cmake_cross[@]}" \
        -DENABLE_TINT2CONF=OFF -DENABLE_EXTRA_THEMES=OFF -DENABLE_RSVG=OFF \
        -DENABLE_SN=OFF -DENABLE_BATTERY=OFF -DENABLE_UEVENT=OFF
    cmake --build build -j"$JOBS"
    cmake --install build
)

# ── Assemble the relocatable prefix ───────────────────────────────────────────
STAGE=/tmp/nested-session
rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/lib"
cp "$PREFIX/bin/openbox" "$PREFIX/bin/picom" "$PREFIX/bin/tint2" "$STAGE/bin/"
# openbox's own libobrender/libobt.
cp -a "$PREFIX"/lib/libob*.so* "$STAGE/lib/"

# Libraries that must come from the host (a trimmed AppImage excludelist).
is_host_library() {
    case "$1" in
        ld-linux*|libc.so*|libm.so*|libdl.so*|libpthread.so*|librt.so*) return 0 ;;
        libresolv.so*|libutil.so*|libgcc_s.so*|libstdc++.so*) return 0 ;;
        libX11.so*|libX11-xcb.so*|libxcb.so*) return 0 ;;
        libGL.so*|libGLX*|libEGL*|libGLdispatch*|libdrm.so*|libgbm.so*) return 0 ;;
        libfontconfig.so*|libfreetype.so*|libharfbuzz.so*|libexpat.so*|libz.so*) return 0 ;;
        *) return 1 ;;
    esac
}

# Every library they need, directly or through another bundled one. Read from
# the ELF headers (readelf), not by ldd, which runs the binary's own loader
# and so cannot look at a cross-compiled one. Searched like the loader would:
# the build prefix first, so openbox's own libraries are found, then the
# target architecture's system directories.
find_library() {
    local dir
    for dir in "$PREFIX/lib" "/usr/lib/$TRIPLE" "/lib/$TRIPLE" /usr/lib /lib; do
        if [ -e "$dir/$1" ]; then
            echo "$dir/$1"
            return 0
        fi
    done
    echo "error: $1 not found for $TRIPLE" >&2
    return 1
}
# Host libraries are walked too, without being copied, as ldd would: what
# they pull in that isn't on the host list itself (libXau, via libxcb) is.
declare -A seen=()
pending=()
for binary in "$STAGE"/bin/* "$STAGE"/lib/libob*.so*; do
    [ -L "$binary" ] || pending+=("$binary")
done
while [ "${#pending[@]}" -gt 0 ]; do
    binary="${pending[0]}"
    pending=("${pending[@]:1}")
    for name in $(readelf -d "$binary" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p'); do
        [ -n "${seen[$name]:-}" ] && continue
        seen[$name]=1
        path="$(find_library "$name")"
        if ! is_host_library "$name"; then
            [ -e "$STAGE/lib/$name" ] || cp -L "$path" "$STAGE/lib/$name"
        fi
        pending+=("$path")
    done
done

for binary in "$STAGE"/bin/*; do
    patchelf --set-rpath '$ORIGIN/../lib' "$binary"
done
for library in "$STAGE"/lib/*; do
    [ -L "$library" ] && continue
    patchelf --set-rpath '$ORIGIN' "$library"
done
$TRIPLE-strip --strip-unneeded "$STAGE"/bin/* "$STAGE"/lib/* 2>/dev/null || true

# Licences travel with the binaries.
mkdir -p "$STAGE/licenses"
cp "$SRC/openbox/COPYING" "$STAGE/licenses/openbox-COPYING"
cp "$SRC/tint2/COPYING" "$STAGE/licenses/tint2-COPYING"
cp -r "$SRC/picom/LICENSES" "$STAGE/licenses/picom"
cp "$SRC/libconfig/COPYING.LIB" "$STAGE/licenses/libconfig-COPYING.LIB"

rm -rf "/out/$OUT_NAME"
cp -a "$STAGE" "/out/$OUT_NAME"
echo ">>> bundled libraries:"
ls "/out/$OUT_NAME/lib"
