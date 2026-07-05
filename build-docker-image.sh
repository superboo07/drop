#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

IMAGE_TAG="drop:custom"
OUTPUT_FILE="drop-custom.tar.gz"

BUILD_DROP_VERSION="$(git describe --tags --always)"
BUILD_GIT_REF="$(git rev-parse HEAD)"

docker build -t "$IMAGE_TAG" \
  --build-arg BUILD_DROP_VERSION="$BUILD_DROP_VERSION" \
  --build-arg BUILD_GIT_REF="$BUILD_GIT_REF" \
  .

docker save "$IMAGE_TAG" | gzip > "$OUTPUT_FILE"

echo "Built $IMAGE_TAG and saved it to $(pwd)/$OUTPUT_FILE"
