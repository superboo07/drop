#!/usr/bin/env bash
# Builds openbox, picom and tint2 (the submodules in this directory) into a
# self-contained, relocatable prefix for the nested session:
#
#   desktop/vendor/out/nested-session/
#     bin/   openbox picom tint2       (RPATH $ORIGIN/../lib)
#     lib/   their shared libraries    (RPATH $ORIGIN)
#
# Usage:
#   bash desktop/vendor/build.sh
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
OUT_DIR="$SCRIPT_DIR/out/nested-session"
BUILDER_IMAGE="drop-nested-session-builder"

# ── Docker wrapper ─────────────────────────────────────────────────────────────
if [ -z "${DROP_IN_DOCKER:-}" ]; then
    for module in openbox picom tint2 libconfig; do
        if [ ! -e "$SCRIPT_DIR/$module/.git" ]; then
            echo "error: $VENDOR_DIR/$module is empty - run" >&2
            echo "  git submodule update --init $VENDOR_DIR" >&2
            exit 1
        fi
    done

    echo ">>> Building nested-session builder image..."
    docker build -t "$BUILDER_IMAGE" "$SCRIPT_DIR"

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
    echo ">>> Building openbox, picom and tint2 inside Docker..."
    mkdir -p "$SCRIPT_DIR/out"
    docker run --rm \
        "${user_args[@]}" \
        -e DROP_IN_DOCKER=1 \
        -e HOME=/tmp \
        -v "$REPO_ROOT":/workspace:ro \
        -v "$SCRIPT_DIR/out":/out \
        -w /workspace \
        "$BUILDER_IMAGE" \
        bash "$VENDOR_DIR/build.sh"
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

rm -rf "$BUILD" "$PREFIX"
mkdir -p "$BUILD" "$PREFIX"
# -L: the submodules' .git entries are files pointing into the superproject's
# .git/modules, which isn't useful in a copy - drop them rather than copy a
# dangling pointer that would make meson's `git describe` complain.
for module in openbox picom tint2; do
    cp -a "$SRC/$module" "$BUILD/$module"
    rm -f "$BUILD/$module/.git"
done
# picom needs libconfig >= 1.7 and mantic ships 1.5, so picom falls back to
# its libconfig subproject (statically linked). Its wrap would git-clone that
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
        -Dopengl=false -Ddbus=false -Dregex=false \
        -Dcompton=false -Dwith_docs=false
    ninja -C build -j"$JOBS"
    ninja -C build install
)

echo ">>> tint2"
(
    cd "$BUILD/tint2"
    cmake -S . -B build -DCMAKE_INSTALL_PREFIX="$PREFIX" -DCMAKE_BUILD_TYPE=Release \
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

# Resolve against the build prefix first so openbox's own libraries are found.
export LD_LIBRARY_PATH="$PREFIX/lib"
for binary in "$STAGE"/bin/* "$STAGE"/lib/libob*.so*; do
    [ -L "$binary" ] && continue
    ldd "$binary" | awk '/=> \// { print $1, $3 }' | while read -r name path; do
        is_host_library "$name" && continue
        [ -e "$STAGE/lib/$name" ] || cp -L "$path" "$STAGE/lib/$name"
    done
done

for binary in "$STAGE"/bin/*; do
    patchelf --set-rpath '$ORIGIN/../lib' "$binary"
done
for library in "$STAGE"/lib/*; do
    [ -L "$library" ] && continue
    patchelf --set-rpath '$ORIGIN' "$library"
done
strip --strip-unneeded "$STAGE"/bin/* "$STAGE"/lib/* 2>/dev/null || true

# Licences travel with the binaries.
mkdir -p "$STAGE/licenses"
cp "$SRC/openbox/COPYING" "$STAGE/licenses/openbox-COPYING"
cp "$SRC/tint2/COPYING" "$STAGE/licenses/tint2-COPYING"
cp -r "$SRC/picom/LICENSES" "$STAGE/licenses/picom"
cp "$SRC/libconfig/COPYING.LIB" "$STAGE/licenses/libconfig-COPYING.LIB"

rm -rf /out/nested-session
cp -a "$STAGE" /out/nested-session
echo ">>> bundled libraries:"
ls /out/nested-session/lib
