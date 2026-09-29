# syntax=docker/dockerfile:1

# Layers here are ordered so an edit only rebuilds what depends on it: every
# stage copies just the files it needs, as late as it can. The `--mount=type=cache`
# directories (pnpm store, cargo registry, torrential's target dir) persist
# between builds on the builder, outside the image, so a source change
# recompiles only the code that changed rather than every dependency.

# Pinned to bookworm so the glibc here matches the torrential build stage
# and the libarchive runtime package is named `libarchive13` (trixie renames it to libarchive13t64).
FROM node:lts-bookworm-slim AS base
ENV PNPM_HOME="/pnpm"
ENV PATH="$PNPM_HOME:$PATH"
RUN corepack enable
WORKDIR /app

## so corepack knows pnpm's version. Only package.json: anything more and
## every source edit would invalidate this stage and all the ones built on it,
## runtime image included.
COPY package.json ./
## prevent prompt to download
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
## setup for offline
RUN corepack pack
## don't call out to network anymore
ENV COREPACK_ENABLE_NETWORK=0

### INSTALL DEPS ONCE
## Just the lockfile and workspace manifests, so this only reruns when
## dependencies change.
FROM base AS deps
COPY pnpm-lock.yaml pnpm-workspace.yaml ./
COPY server/package.json ./server/
COPY libraries/base/package.json ./libraries/base/
RUN --mount=type=cache,id=drop-pnpm-store,target=/pnpm/store \
    pnpm install --frozen-lockfile --ignore-scripts --store-dir /pnpm/store

### BUILD TORRENTIAL
# Bookworm-pinned to match the runtime image's glibc (a trixie build would not run on bookworm).
FROM rustlang/rust:nightly-bookworm-slim AS torrential-build
## libarchive-dev + pkg-config let libarchive3-sys link libarchive dynamically (glibc).
## protobuf-compiler is kept for parity (torrential's build.rs uses a vendored protoc).
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libarchive-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
## torrential and the path crates it depends on (droplet, droplet_types, libarchive)
COPY libraries ./libraries
COPY torrential ./torrential
## The registry and target dir are cache mounts: without them every build
## starts with an empty registry, and freshly unpacked crate sources get new
## mtimes, so cargo sees every dependency as changed and rebuilds the whole
## tree. A cache mount isn't part of the image, so the binary is copied out in
## the same step.
RUN --mount=type=cache,id=drop-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=drop-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=drop-torrential-target,target=/build/torrential/target \
    cargo build --release --manifest-path ./torrential/Cargo.toml \
    && cp ./torrential/target/release/torrential /usr/local/bin/torrential

### BUILD APP
FROM deps AS build-system

ENV NODE_ENV=production
ENV NUXT_TELEMETRY_DISABLED=1

## add git so drop can determine its git ref at build
RUN apt-get update && apt-get install -y --no-install-recommends git \
    && rm -rf /var/lib/apt/lists/*

## rest of project files, over the installed deps (.dockerignore keeps the
## host's node_modules out)
COPY . .

ARG BUILD_DROP_VERSION
ARG BUILD_GIT_REF

## build
RUN pnpm run --filter=drop postinstall && pnpm run --filter=drop build


# create run environment for Drop
FROM base AS run-system

ENV NODE_ENV=production
ENV NUXT_TELEMETRY_DISABLED=1

# RUN --mount=type=cache,target=/root/.yarn YARN_CACHE_FOLDER=/root/.yarn yarn add --network-timeout 1000000 --no-lockfile --ignore-scripts prisma@6.11.1
## runtime deps:
##  - libarchive13: torrential now links libarchive dynamically (glibc build)
##  - p7zip-full: provides the 7z CLI
##  - nginx: front-end proxy
##  - openssl + ca-certificates: required by Prisma's query engine on Debian
## pnpm itself is provided by corepack (enabled in the base stage)
## upgrade picks up Debian security fixes (e.g. tzdata) newer than the base image.
RUN apt-get update && apt-get upgrade -y \
    && apt-get install -y --no-install-recommends \
    libarchive13 \
    p7zip-full \
    nginx \
    openssl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
## The node image's npm is never used here (pnpm comes from corepack) and
## carries its own vulnerable copies of tar, undici etc.
RUN rm -rf /usr/local/lib/node_modules/npm /usr/local/bin/npm /usr/local/bin/npx
## The Prisma CLI for migrations, from a manifest rather than a bare global
## install so its overrides can patch the dependencies prisma pins. --dir
## keeps the cwd at /app, where corepack reads the pnpm version.
COPY server/build/prisma-cli/package.json /opt/prisma-cli/
RUN pnpm install --dir /opt/prisma-cli
ENV PATH="/opt/prisma-cli/node_modules/.bin:$PATH"
# init prisma to download all required files
RUN pnpm prisma init

COPY --from=build-system /app/server/prisma.config.ts ./
COPY --from=build-system /app/server/.output ./app
COPY --from=build-system /app/server/prisma ./prisma
COPY --from=build-system /app/server/build ./startup
COPY --from=build-system /app/server/build/nginx.conf /nginx.conf
# The torrential service resolves its binary by scanning the cwd (/app) for
# `torrential` before falling back to PATH, so it must not be put there.
COPY --from=torrential-build /usr/local/bin/torrential /usr/bin/

ENV LIBRARY="/library"
ENV DATA="/data"
ENV NGINX_CONFIG="/nginx.conf"
# Nuxt's port
ENV PORT=4000

CMD ["sh", "/app/startup/launch.sh"]
