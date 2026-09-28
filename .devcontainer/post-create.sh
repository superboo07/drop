#!/usr/bin/env bash
# Runs once, after the container is created. Idempotent and never destructive:
# nothing that already exists is replaced.
set -euo pipefail

cd "$(dirname "$0")/.."

# The tree is a bind mount from the host, so git sees an owner that is not the
# user running it.
git config --global --add safe.directory "$PWD" || true

# The game library. The server's default LIBRARY is ./.data/library relative to
# server/ (server/server/internal/config/sys-conf.ts), where `pnpm dev` runs.
# The library is mounted at its own path (docker-compose.yml), so link it there
# -- the link target is the same path on the host, so it is valid outside the
# container too. An existing library there (a host `pnpm dev` made one) is left
# alone.
library_link="server/.data/library"
if [ -n "${DROP_LIBRARY_DIR:-}" ] && [ -n "$(ls -A "$DROP_LIBRARY_DIR" 2>/dev/null)" ] \
    && [ ! -e "$library_link" ] && [ ! -L "$library_link" ]; then
    mkdir -p "$(dirname "$library_link")"
    ln -s "$DROP_LIBRARY_DIR" "$library_link"
    echo "Game library linked: $library_link -> $DROP_LIBRARY_DIR"
fi

# Everything to tell you -- a missing library, git identity, signing key, the
# commands -- is in welcome.sh, which checks the state as it is *now* and runs
# again in every terminal and before claude.sh starts Claude. Output from this
# script only reaches the creation log, which nobody sees twice.
bash .devcontainer/welcome.sh || true
