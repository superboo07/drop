# Builder for the nested session's window manager, compositor and taskbar
# (openbox, picom, tint2, plus libconfig for picom - the submodules next to
# this file).
#
# Not meant to be run directly - build.sh handles everything:
#   bash desktop/vendor/build.sh
#
# Same base as ../Dockerfile.build so the binaries link against the same
# glibc as the rest of the AppImage.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# Toolchains: autotools for openbox, meson for picom, cmake for tint2.
# Libraries: only what the trimmed builds in build.sh actually use - picom is
# built without OpenGL, D-Bus and PCRE, tint2 without tint2conf (GTK), rsvg,
# startup-notification and the battery/uevent plugins, openbox without
# imlib2, rsvg, startup-notification and session management.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates build-essential pkg-config patch patchelf file \
        autoconf automake libtool autopoint gettext \
        meson ninja-build cmake \
        libglib2.0-dev libpango1.0-dev libcairo2-dev libxml2-dev \
        libx11-dev libx11-xcb-dev libxext-dev libxrandr-dev libxinerama-dev \
        libxcursor-dev libxft-dev libxcomposite-dev libxdamage-dev \
        libxrender-dev libimlib2-dev \
        libxcb1-dev libxcb-composite0-dev libxcb-damage0-dev libxcb-glx0-dev \
        libxcb-present-dev libxcb-randr0-dev libxcb-render0-dev \
        libxcb-shape0-dev libxcb-sync-dev libxcb-xfixes0-dev \
        libxcb-image0-dev libxcb-render-util0-dev libxcb-util-dev \
        libpixman-1-dev libev-dev uthash-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace
