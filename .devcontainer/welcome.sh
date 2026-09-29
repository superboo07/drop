#!/usr/bin/env bash
# What still needs doing in this container, and how to work in it. Read-only:
# it checks the current state and prints, nothing else, so it is safe to run
# any number of times.
#
# post-create.sh's own output only ever reaches the creation log (VS Code hides
# it, `devcontainer up` buries it in the build log, and a reused container never
# runs it again), so this is what actually puts it in front of you:
#   - claude.sh runs it before starting Claude,
#   - the image's .bashrc runs it in every interactive terminal (full the first
#     time in a container, only the to-do items after that),
#   - post-create.sh runs it last, for the creation log.
#
#   welcome.sh           to-do items + the command reference
#   welcome.sh --brief   to-do items only
#
# Exits 10 when there is something to do, so callers can wait for a keypress.
set -uo pipefail

cd "$(dirname "$0")/.."

brief=0
[ "${1:-}" = "--brief" ] && brief=1
todo=0

# The game library (post-create.sh links it; this says when it could not).
library_link="server/.data/library"
if [ -n "${DROP_LIBRARY_DIR:-}" ]; then
    if [ ! -d "$DROP_LIBRARY_DIR" ] || [ -z "$(ls -A "$DROP_LIBRARY_DIR" 2>/dev/null)" ]; then
        todo=1
        echo "  ! DROP_LIBRARY_DIR=$DROP_LIBRARY_DIR is empty or not a directory -- check the path in .env."
    elif [ "$(readlink "$library_link" 2>/dev/null || true)" != "$DROP_LIBRARY_DIR" ]; then
        echo "  ! $library_link exists and is not a link to $DROP_LIBRARY_DIR; left as it is."
    fi
elif [ ! -e "$library_link" ]; then
    todo=1
    cat <<'MSG'
  ------------------------------------------------------------------
  No game library is mounted.

  Copy .env.example to .env -- at the top of the repository, not in
  .devcontainer/ -- point DROP_LIBRARY_DIR at your library
  (/{game}/{version}/...), and rebuild the container. Everything else
  works without it; the server just has nothing to import.
  ------------------------------------------------------------------
MSG
fi

# The nested session's tools. desktop/vendor/build.sh builds them in a
# container, which works in here through podman (the Dockerfile's podman layer).
if [ ! -x desktop/vendor/out/nested-session/bin/openbox ]; then
    cat <<'MSG'

  The nested session's openbox/picom/tint2 are not built, so its tests
  will skip themselves. Build them (in a container, via podman, in here):
      git submodule update --init desktop/vendor
      bash desktop/vendor/build.sh

MSG
fi

# The identity git commits with. Nothing is copied from the host: ~/.gitconfig
# and ~/.ssh are volumes this container owns (see docker-compose.yml), so the
# first container asks once and every rebuild after it stays quiet.
if ! git config --global --get user.email >/dev/null 2>&1; then
    todo=1
    cat <<'MSG'

  ------------------------------------------------------------------
  git has no identity in this container yet. It is not taken from your
  host config on purpose -- set it once and the volume keeps it:

      git config --global user.name  "you"
      git config --global user.email "you@example.com"

  For signed commits, put a key in /root/.ssh (also a volume, also
  yours) and point git at it:

      ssh-keygen -t ed25519 -f /root/.ssh/id_ed25519
      git config --global gpg.format ssh
      git config --global user.signingkey /root/.ssh/id_ed25519.pub
      git config --global commit.gpgsign true

  Then print the public key and add it to your git server (GitHub:
  Settings > SSH and GPG keys > New SSH key, key type "Signing Key";
  Gitea/Forgejo/GitLab have the same under SSH keys). Add it again as
  an "Authentication Key" if you also push over SSH with it:

      cat /root/.ssh/id_ed25519.pub
  ------------------------------------------------------------------

MSG
elif [ -f /root/.ssh/id_ed25519.pub ] \
    && [ -z "$(git config --global --get user.signingkey 2>/dev/null)" ]; then
    # A key is in the volume but git is not signing with it yet.
    cat <<'MSG'

  A key exists in /root/.ssh but git does not sign with it. To use it:
      git config --global gpg.format ssh
      git config --global user.signingkey /root/.ssh/id_ed25519.pub
      git config --global commit.gpgsign true
  and add the output of this to your git server as a Signing Key:
      cat /root/.ssh/id_ed25519.pub

MSG
fi

if [ "$brief" = 0 ]; then
    cat <<'MSG'

Ready. Postgres is the `db` service (DATABASE_URL is set). Commands:
  pnpm install                                    # the whole workspace
  cd server && pnpm prisma migrate deploy && pnpm dev    # http://localhost:4000
  pnpm --filter=drop run typecheck | lint
  cargo clippy --manifest-path torrential/Cargo.toml     # or cli/, libraries/droplet/
  cargo test --manifest-path libraries/droplet/Cargo.toml --all-features --all
  cd desktop && pnpm tauri dev                    # opens on your desktop (Wayland)
  cargo clippy --manifest-path desktop/src-tauri/Cargo.toml
  (cd backend && go build ./core/... && GOWORK=off go build .)   # go.work only lists core
Nested-session tests, where nothing reaches your screen (desktop/CLAUDE.md):
  weston --backend=headless --socket=headless --width=1280 --height=800 --idle-time=0 & W=$!
  WAYLAND_DISPLAY=headless cargo test --manifest-path desktop/src-tauri/Cargo.toml -p nested_session
  kill $W
Cargo builds land in target-container/, not each crate's target/.
This list again: bash .devcontainer/welcome.sh
MSG
fi

[ "$todo" = 1 ] && exit 10
exit 0
