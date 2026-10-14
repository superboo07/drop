#!/usr/bin/env bash
# linuxdeploy output plugin that finishes the AppDir and packs it - the one
# and only time the AppImage is compressed.
#
# Tauri's bundler runs linuxdeploy with `--output appimage`, and takes that
# plugin from its tools directory (~/.cache/tauri) as
# linuxdeploy-plugin-appimage.AppImage, downloading it only when the file is
# missing. build_appimage.sh installs this script there under that name before
# every build, so linuxdeploy hands it the finished AppDir instead of packing
# one itself. Changes to the AppDir can only be made here: libwayland exists
# only once linuxdeploy has bundled it, and linuxdeploy would rewrite the
# nested-session tools' runpaths and pull their libraries into usr/lib if they
# were in the AppDir before it ran.
#
# Plugin protocol: linuxdeploy asks for --plugin-api-version and --plugin-type,
# then runs `<plugin> --appdir <dir>` with OUTPUT (the AppImage to write) and
# ARCH set. build_appimage.sh exports the rest: LDAI_RUNTIME_FILE (the AppImage
# runtime) and DROP_NESTED_SESSION_TOOLS (this architecture's openbox/picom/
# tint2 prefix).

set -euo pipefail

case "${1:-}" in
    --plugin-api-version) echo 0; exit 0 ;;
    --plugin-type) echo output; exit 0 ;;
esac

APPDIR=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --appdir) APPDIR="$2"; shift 2 ;;
        --appdir=*) APPDIR="${1#--appdir=}"; shift ;;
        *) shift ;;
    esac
done
for var in APPDIR OUTPUT ARCH LDAI_RUNTIME_FILE DROP_NESTED_SESSION_TOOLS; do
    if [ -z "${!var:-}" ]; then
        echo "error: $var is not set - is this being run by build_appimage.sh's tauri build?" >&2
        exit 1
    fi
done

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

# Vendored umu-run/winetricks (Steam Deck etc. support). These have no distro
# package manager to install umu-launcher/winetricks on, so we bundle
# known-working copies as a fallback. Placed in their own directory (not
# usr/bin) so they don't get caught up in the sanitize step that strips the
# AppImage's usr/bin from PATH before spawning external tools (see
# utils::external_open::sanitize_external_command) -- that step re-adds this
# specific directory back.
echo ">>> Injecting vendored umu-run/winetricks..."
mkdir -p "$APPDIR/usr/libexec/drop-tools"
cp /opt/drop-vendor/umu-run /opt/drop-vendor/winetricks "$APPDIR/usr/libexec/drop-tools/"

# The nested session's window manager, compositor and taskbar, as built by
# vendor/build.sh on the host side of build_appimage.sh. A self-contained
# prefix (own lib/, RUNPATH $ORIGIN/../lib), found by nested_session::tools at
# $APPDIR/usr/libexec/drop-tools/nested-session. In a subdirectory, so the
# PATH entry sanitize_external_command adds for drop-tools doesn't expose
# them to games.
echo ">>> Injecting nested-session tools..."
if [ ! -x "$DROP_NESTED_SESSION_TOOLS/bin/openbox" ]; then
    echo "error: $DROP_NESTED_SESSION_TOOLS is missing - vendor/build.sh should have built it" >&2
    exit 1
fi
rm -rf "$APPDIR/usr/libexec/drop-tools/nested-session"
cp -a "$DROP_NESTED_SESSION_TOOLS" "$APPDIR/usr/libexec/drop-tools/nested-session"

# The runtime is baked into the builder image; without --runtime-file,
# appimagetool downloads it on every build.
echo ">>> Packing AppImage..."
appimagetool --runtime-file "$LDAI_RUNTIME_FILE" "$APPDIR" "$OUTPUT"
