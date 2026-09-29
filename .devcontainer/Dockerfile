# The devcontainer image for the whole Drop monorepo: Node + pnpm for the Nuxt
# server/desktop frontend/sites, Rust nightly for torrential/cli/droplet and the
# Tauri client, Go for backend/, the Tauri and nested-session runtime so the
# desktop client can open a window from in here, and Claude Code.
#
# Postgres is NOT in here -- it is its own compose service (docker-compose.yml).
#
# **Nothing user-supplied is in this image** -- no game library, no server
# data, no API keys (server/.env), no LunaTranslator/Proton. Those are the
# user's and are mounted or live in the (bind-mounted) tree.

# Debian trixie. Its WebKitGTK is current and security-maintained, which is
# the point: this is a browser engine rendering what a remote server sends. In
# here `pnpm tauri dev` runs against the image's own coherent WebKit/libwayland/
# Mesa set, so the AppImage's libwayland mismatch (desktop/build_appimage.sh,
# step 4) cannot happen. The nested session's tools, built on Ubuntu 24.04
# (desktop/vendor/Dockerfile), run here too: trixie's glibc is newer.
FROM debian:trixie-slim

# Build toolchain. build-essential/pkg-config/clang for the Rust -sys crates;
# protobuf-compiler for parity with the root Dockerfile's torrential stage
# (torrential's build.rs vendors protoc, the server uses buf from npm); python3
# for node-gyp and the checksum steps below; git so the server can read its git
# ref (root Dockerfile, build-system stage).
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
	&& apt-get install -y --no-install-recommends \
		build-essential \
		pkg-config \
		clang \
		protobuf-compiler \
		python3 \
		git \
		ca-certificates \
		curl \
		file \
		xz-utils \
		unzip \
	&& apt-get clean && rm -rf /var/lib/apt/lists/*

# Native libraries the crates link against:
#  - libarchive-dev: libarchive3-sys, via droplet-rs, in torrential, cli and the
#    client (CLAUDE.md; droplet-ci.yml / torrential-ci.yml "Install libarchive").
#  - The Tauri 2 Linux set, from desktop/Dockerfile.build and client-release.yml:
#    webkit2gtk/javascriptcoregtk 4.1, appindicator (Debian only packages the
#    ayatana fork, which is what Tauri's docs name for Debian), rsvg, soup3,
#    openssl, patchelf/xdg-utils for the bundler. libdbus-1-dev for libdbus-sys.
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
	&& apt-get install -y --no-install-recommends \
		libarchive-dev \
		libwebkit2gtk-4.1-dev \
		libjavascriptcoregtk-4.1-dev \
		libayatana-appindicator3-dev \
		librsvg2-dev \
		libsoup-3.0-dev \
		libssl-dev \
		libdbus-1-dev \
		patchelf \
		xdg-utils \
	&& apt-get clean && rm -rf /var/lib/apt/lists/*

# What the server spawns or shells out to at run time -- the root Dockerfile's
# run-system stage: nginx (the reverse-proxy service), p7zip-full (the 7z CLI),
# openssl (Prisma). postgresql-client so `psql` can look at the db service.
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
	&& apt-get install -y --no-install-recommends \
		nginx \
		p7zip-full \
		openssl \
		postgresql-client \
	&& apt-get clean && rm -rf /var/lib/apt/lists/*

# Running the desktop client and the nested session in here.
#  - Mesa: the WebKit web process renders on the host GPU through /dev/dri;
#    mesa-utils' `glxinfo -B` says whether it arrived when a window comes up black.
#  - xwayland + weston: the nested session starts a rootful Xwayland, and
#    desktop/CLAUDE.md runs its tests inside a *headless* Weston so nothing lands
#    on your screen. x11-utils (xmessage, xprop) and imagemagick (`import`) are
#    the X clients / screenshot tool those instructions use.
#  - fonts: the vendored openbox/tint2 take fonts from the system (their
#    AppImage excludelist split), and so does WebKit. CJK because the library
#    and the LunaTranslator integration are largely Japanese visual novels.
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
	&& apt-get install -y --no-install-recommends \
		libgl1-mesa-dri libglx-mesa0 libegl-mesa0 libgbm1 mesa-utils \
		xwayland weston x11-utils x11-xserver-utils imagemagick \
		fonts-dejavu-core fonts-noto-cjk \
	&& apt-get clean && rm -rf /var/lib/apt/lists/*

# The editor side.
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
	&& apt-get install -y --no-install-recommends \
		less procps sudo bash-completion openssh-client ripgrep \
	&& apt-get clean && rm -rf /var/lib/apt/lists/*

# Node. server/package.json `engines` wants >=22.16.0 and CI uses lts/*;
# trixie's own nodejs is 20, so it comes from nodejs.org, checked against the
# release's SHASUMS256.txt. `lts` resolves to the newest LTS; anything else is
# a pinned version (e.g. 24.21.0).
ARG NODE_VERSION=lts
RUN set -eu; \
	case "${NODE_VERSION}" in \
		lts) version=$(curl -fsSL https://nodejs.org/dist/index.json \
			| python3 -c "import json,sys; print(next(r['version'] for r in json.load(sys.stdin) if r['lts']))") ;; \
		*) version="v${NODE_VERSION#v}" ;; \
	esac; \
	case "$(uname -m)" in \
		x86_64) arch=x64 ;; \
		aarch64) arch=arm64 ;; \
		*) echo "no Node build for $(uname -m)" >&2; exit 1 ;; \
	esac; \
	tarball="node-$version-linux-$arch.tar.xz"; \
	cd /tmp; \
	curl -fsSLO "https://nodejs.org/dist/$version/$tarball"; \
	curl -fsSL "https://nodejs.org/dist/$version/SHASUMS256.txt" | grep " $tarball\$" | sha256sum -c -; \
	tar -xJf "$tarball" -C /usr/local --strip-components=1 --no-same-owner; \
	rm -f "$tarball"; \
	node --version

# pnpm, at the exact version package.json's `packageManager` pins, through
# corepack. Newer Node releases no longer bundle corepack, so it is installed
# explicitly rather than assumed.
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0
RUN npm install -g corepack@latest && corepack enable

# Rust nightly: desktop/src-tauri and cli pin `channel = "nightly"` in
# rust-toolchain.toml, and CI runs torrential/droplet on nightly too
# (dtolnay/rust-toolchain@nightly). rustfmt + clippy are what droplet-ci.yml
# checks with. rustup-init is verified against its published .sha256.
ENV RUSTUP_HOME=/usr/local/rustup \
	CARGO_HOME=/usr/local/cargo \
	PATH=/usr/local/cargo/bin:$PATH
RUN set -eu; \
	target="$(uname -m)-unknown-linux-gnu"; \
	base="https://static.rust-lang.org/rustup/dist/$target"; \
	cd /tmp; \
	curl -fsSLO "$base/rustup-init"; \
	echo "$(curl -fsSL "$base/rustup-init.sha256" | cut -d' ' -f1)  rustup-init" | sha256sum -c -; \
	chmod +x rustup-init; \
	./rustup-init -y --no-modify-path --profile minimal --default-toolchain nightly \
		--component rustfmt,clippy; \
	rm -f rustup-init; \
	rustc --version

# Go, for backend/ (backend/go.mod: `go 1.26.1`); trixie's golang is 1.24.
# Checked against the sha256 go.dev publishes for the archive.
ARG GO_VERSION=1.26.8
ENV PATH=/usr/local/go/bin:$PATH
RUN set -eu; \
	case "$(uname -m)" in \
		x86_64) arch=amd64 ;; \
		aarch64) arch=arm64 ;; \
		*) echo "no Go build for $(uname -m)" >&2; exit 1 ;; \
	esac; \
	file="go${GO_VERSION}.linux-$arch.tar.gz"; \
	cd /tmp; \
	curl -fsSLO "https://go.dev/dl/$file"; \
	curl -fsSL 'https://go.dev/dl/?mode=json&include=all' \
		| python3 -c "import json,sys; print(next(f['sha256'] for r in json.load(sys.stdin) for f in r['files'] if f['filename'] == '$file'))" \
		| sed "s|\$|  $file|" | sha256sum -c -; \
	tar -xzf "$file" -C /usr/local; \
	rm -f "$file"; \
	go version

# Fail the image build, not the first `pnpm install`/`cargo build` inside it.
RUN set -eu; \
	fail() { echo "base image no longer satisfies: $*" >&2; exit 1; }; \
	pkg-config --exists webkit2gtk-4.1 || fail "Tauri 2's webkit2gtk-4.1 (desktop/Dockerfile.build, client-release.yml)"; \
	pkg-config --exists libarchive || fail "libarchive-dev (CLAUDE.md, droplet-ci.yml)"; \
	node -e 'const [a,b]=process.versions.node.split(".").map(Number); process.exit(a>22||(a==22&&b>=16)?0:1)' \
		|| fail "Node >=22.16.0 (server/package.json engines)"; \
	rustc --version | grep -q nightly || fail "Rust nightly (rust-toolchain.toml)"; \
	go version | grep -Eq 'go1\.(2[6-9]|[3-9][0-9])' || fail "Go >=1.26.1 (backend/go.mod)"; \
	command -v nginx >/dev/null || fail "nginx (server services, root Dockerfile)"

# **Container builds work in here, with podman** -- the project's own Dockerfiles
# and scripts that call `docker build`/`docker run` (podman-docker is the
# `docker` command). There is no docker socket: handing one in would give the
# container the host's daemon. Podman runs as this container's root, which the
# rootless host daemon maps to you, so nested builds get no more than you have.
# docker-compose.yml supplies what it needs (SYS_ADMIN inside our user
# namespace, seccomp unconfined for pivot_root/keyctl, a storage volume); the
# config below covers what a container cannot do:
#  - netns=host: netavark cannot create bridges (netlink EPERM); builds and
#    runs share this container's network instead.
#  - cgroups=disabled: /sys/fs/cgroup is read-only in here.
#  - keyring=false: session keyrings are denied.
#  - short names resolve on Docker Hub, as `FROM debian:trixie` does in Docker.
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
	&& apt-get install -y --no-install-recommends \
		podman podman-docker crun uidmap \
	&& apt-get clean && rm -rf /var/lib/apt/lists/* \
	&& printf '%s\n' '[containers]' 'netns = "host"' 'cgroups = "disabled"' 'keyring = false' \
		'[engine]' 'cgroup_manager = "cgroupfs"' > /etc/containers/containers.conf \
	&& printf '%s\n' 'unqualified-search-registries = ["docker.io"]' 'short-name-mode = "permissive"' \
		> /etc/containers/registries.conf.d/50-docker-hub.conf \
	&& touch /etc/containers/nodocker

# XDG_RUNTIME_DIR for root; docker-compose.yml binds the host's Wayland socket
# into it. The nested session also writes its per-session config under it.
# Created here so it has the 0700 the spec requires.
RUN mkdir -p /run/user/0 && chmod 700 /run/user/0

# Claude Code, from Anthropic's release channel (the npm install is deprecated
# and the binary is self-contained). `stable`/`latest` are moving channels;
# anything else is a pinned version. The checksum comes from the release
# manifest, so a truncated or substituted download fails the build.
ARG CLAUDE_CODE_VERSION=latest
RUN set -eu; \
	base=https://downloads.claude.ai/claude-code-releases; \
	case "${CLAUDE_CODE_VERSION}" in \
		stable|latest) version=$(curl -fsSL "$base/${CLAUDE_CODE_VERSION}") ;; \
		*) version="${CLAUDE_CODE_VERSION}" ;; \
	esac; \
	case "$(uname -m)" in \
		x86_64) platform=linux-x64 ;; \
		aarch64) platform=linux-arm64 ;; \
		*) echo "no Claude Code build for $(uname -m)" >&2; exit 1 ;; \
	esac; \
	curl -fsSL "$base/$version/manifest.json" \
		| python3 -c "import json,sys; print(json.load(sys.stdin)['platforms']['$platform']['checksum'])" \
		> /tmp/claude.sha256; \
	curl -fsSL -o /usr/local/bin/claude "$base/$version/$platform/claude"; \
	echo "$(cat /tmp/claude.sha256)  /usr/local/bin/claude" | sha256sum -c -; \
	chmod +x /usr/local/bin/claude; \
	rm -f /tmp/claude.sha256; \
	claude --version

# The image owns the version; rebuild the container to move it.
ENV DISABLE_AUTOUPDATER=1
ENV DISABLE_INSTALLATION_CHECKS=1

# Mount points for the volumes docker-compose.yml puts here: the SSH directory
# you put your own key in, git's global config, and Claude Code's state. Created
# in the image so the volumes inherit these modes -- .ssh in particular is 0700
# or ssh refuses to use what is in it. **Nothing is copied from the host**.
RUN mkdir -p /root/.ssh /root/.config/git /root/.claude /root/.local/share/pnpm/store \
	&& chmod 700 /root/.ssh

RUN mkdir -p /commandhistory && touch /commandhistory/.bash_history
ENV HISTFILE=/commandhistory/.bash_history

# The to-do list and command reference (welcome.sh) in every interactive
# terminal -- post-create output only reaches a creation log nobody reads. The
# whole thing the first time in a container, only the to-do items after that.
# Non-interactive shells (`devcontainer exec claude`, lifecycle hooks) skip it.
RUN cat >> /root/.bashrc <<'SH'

if [[ $- == *i* ]] && [ -f /workspaces/drop/.devcontainer/welcome.sh ]; then
	if [ -e /tmp/.welcome-shown ]; then
		bash /workspaces/drop/.devcontainer/welcome.sh --brief
	else
		bash /workspaces/drop/.devcontainer/welcome.sh; touch /tmp/.welcome-shown
	fi
fi
SH

ENV DEVCONTAINER=true
