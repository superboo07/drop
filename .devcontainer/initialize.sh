#!/usr/bin/env bash
# **Runs on the host, before the container exists** (devcontainer.json's
# initializeCommand -- VS Code, the devcontainer CLI and claude.sh alike).
#
# 1. Compose looks for .env beside the compose file; the project keeps it at the
#    top of the tree. A link rather than a copy so there is one .env, made only
#    when there is one to link to. Both paths are gitignored by `*.env`.
#
# 2. The dev container tooling runs `podman compose`, which hands the work to an
#    external provider: docker-compose if it is installed, else podman-compose.
#    docker-compose talks to Podman through its API socket, which is not running
#    unless enabled, and then `up` fails with "failed to connect to the docker
#    API at unix:///run/user/<uid>/podman/podman.sock". Say so here, with the
#    fix, instead of letting that surface from deep in the tooling's log.
#
# 3. The image is built here, with a plain `podman build` (rootless, as you),
#    and the container config only names it. The dev container tooling never
#    builds it: its generated Dockerfile-with-features build lives outside the
#    build context (Docker's buildx bake refuses it outright), and a plain build
#    of this directory is the same on every engine. Layer caching keeps it
#    quick when nothing changed; "Rebuild Container" runs it again.
#    CLAUDE_CODE_VERSION (stable, latest, or a version), NODE_VERSION (lts or a
#    version) and GO_VERSION in the host environment are passed through to the
#    Dockerfile's ARGs; add --no-cache by hand to re-fetch a moving channel.
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v podman >/dev/null 2>&1; then
    echo "error: this dev container is run by rootless Podman, and podman is not on PATH." >&2
    exit 1
fi

if [ -f .env ]; then
    ln -sfn ../.env .devcontainer/.env
fi

provider=$(PODMAN_COMPOSE_WARNING_LOGS=true podman compose version 2>&1 | sed -n 's/.*external compose provider "\([^"]*\)".*/\1/p' | head -n1)
if [ -z "$provider" ]; then
    cat >&2 <<'MSG'
error: `podman compose` found no compose provider. Install podman-compose
       (or docker-compose) on the host -- the one thing besides podman this
       container needs, because it is a compose project.
MSG
    exit 1
fi
case "$(basename "$provider")" in
    docker-compose)
        if ! podman --remote info >/dev/null 2>&1; then
            cat >&2 <<MSG
error: \`podman compose\` uses $provider, which needs Podman's API socket,
       and it is not running. Best: make podman use podman-compose (no
       socket) for every caller -- VS Code, claude.sh and your terminal alike
       -- in ~/.config/containers/containers.conf:
           [engine]
           compose_providers = ["podman-compose"]
       One provider everywhere also matters because the two name the compose
       project differently, and VS Code and claude.sh would otherwise each
       start their own container (and their own Postgres on drop-pgdata).
       Or start the socket (and keep it across logins):
           systemctl --user enable --now podman.socket
MSG
            exit 1
        fi
        ;;
esac

build_args=()
for arg in CLAUDE_CODE_VERSION NODE_VERSION GO_VERSION; do
    if [ -n "${!arg:-}" ]; then
        build_args+=(--build-arg "$arg=${!arg}")
    fi
done

podman build "${build_args[@]}" -t localhost/drop-devcontainer:latest .devcontainer
