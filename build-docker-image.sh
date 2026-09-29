#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

# Fully qualified: podman (which `docker` is in the dev container) otherwise
# stores an unqualified name as localhost/drop:custom, and `docker save` writes
# that into the archive. Docker loads docker.io/library/drop:custom as plain
# drop:custom.
IMAGE_TAG="docker.io/library/drop:custom"
OUTPUT_FILE="drop-custom.tar.gz"

BUILD_DROP_VERSION="$(git describe --tags --always)"
BUILD_GIT_REF="$(git rev-parse HEAD)"

docker build -t "$IMAGE_TAG" \
  --build-arg BUILD_DROP_VERSION="$BUILD_DROP_VERSION" \
  --build-arg BUILD_GIT_REF="$BUILD_GIT_REF" \
  .

docker save "$IMAGE_TAG" | gzip > "$OUTPUT_FILE"

echo "Built drop:custom and saved it to $(pwd)/$OUTPUT_FILE"
