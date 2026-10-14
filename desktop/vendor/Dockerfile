# Builder for the nested session's window manager, compositor and taskbar
# (openbox, picom, tint2, plus libconfig for picom - the submodules next to
# this file).
#
# Not meant to be run directly - build.sh handles everything:
#   bash desktop/vendor/build.sh
#
# Same base as ../Dockerfile.build so the binaries link against the same
# glibc as the rest of the AppImage. Like it, one stage per target
# architecture on an x86_64 base: `amd64` builds natively, `arm64`
# cross-compiles against Ubuntu's arm64 libraries (dpkg multiarch).

# Libraries: only what the trimmed builds in build.sh actually use - picom is
# built without OpenGL, D-Bus and PCRE, tint2 without tint2conf (GTK), rsvg,
# startup-notification and the battery/uevent plugins, openbox without
# imlib2, rsvg, startup-notification and session management. Installed for
# the target architecture by its stage below.
ARG LIBS="libglib2.0-dev libpango1.0-dev libcairo2-dev libxml2-dev \
        libx11-dev libx11-xcb-dev libxext-dev libxrandr-dev libxinerama-dev \
        libxcursor-dev libxft-dev libxcomposite-dev libxdamage-dev \
        libxrender-dev libimlib2-dev \
        libxcb1-dev libxcb-composite0-dev libxcb-damage0-dev libxcb-glx0-dev \
        libxcb-present-dev libxcb-randr0-dev libxcb-render0-dev \
        libxcb-shape0-dev libxcb-sync-dev libxcb-xfixes0-dev \
        libxcb-image0-dev libxcb-render-util0-dev libxcb-util-dev \
        libpixman-1-dev libev-dev uthash-dev"

FROM docker.io/library/ubuntu:24.04 AS base

ENV DEBIAN_FRONTEND=noninteractive

# Toolchains: autotools for openbox, meson for picom, cmake for tint2.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates build-essential pkg-config patch patchelf file \
        autoconf automake libtool autopoint gettext \
        meson ninja-build cmake \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

FROM base AS amd64
ARG LIBS

RUN apt-get update && apt-get install -y --no-install-recommends $LIBS \
    && rm -rf /var/lib/apt/lists/*

FROM base AS arm64
ARG LIBS

# arm64 packages come from Ubuntu's ports archive; the main one has none.
# qemu-user-static is never run here: libglib2.0-dev:arm64 depends on
# "python3 | qemu-user | qemu-user-static", for its arm64 helper programs,
# and without it apt installs arm64 Python - which cannot run here, so its
# postinst fails the install.
RUN sed -i '/^Components:/a Architectures: amd64' /etc/apt/sources.list.d/ubuntu.sources \
    && printf '%s\n' \
        'Types: deb' \
        'URIs: http://ports.ubuntu.com/ubuntu-ports/' \
        'Suites: noble noble-updates noble-security' \
        'Components: main universe restricted multiverse' \
        'Architectures: arm64' \
        'Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg' \
        > /etc/apt/sources.list.d/ubuntu-ports.sources \
    && dpkg --add-architecture arm64 \
    && apt-get update && apt-get install -y --no-install-recommends \
        crossbuild-essential-arm64 qemu-user-static \
        $(for lib in $LIBS; do echo "$lib:arm64"; done) \
    && rm -rf /var/lib/apt/lists/*
