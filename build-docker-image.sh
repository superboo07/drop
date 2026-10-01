#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

# A local-only image, loaded with `docker load` and never pushed anywhere, so
# the archive names it plain drop:custom, with no registry.
IMAGE_TAG="drop:custom"
OUTPUT_FILE="drop-custom.tar.gz"

BUILD_DROP_VERSION="$(git describe --tags --always --dirty)"
BUILD_GIT_REF="$(git rev-parse HEAD)"

# Podman (`docker` in the dev container) builds one stage at a time unless
# told otherwise; BuildKit already runs independent stages in parallel. Let
# torrential, the Nuxt build, the audit and the runtime image build at once.
PARALLEL=()
if docker --version 2>/dev/null | grep -qi podman; then
  PARALLEL=(--jobs 0)
fi

docker build -t "$IMAGE_TAG" "${PARALLEL[@]}" \
  --build-arg BUILD_DROP_VERSION="$BUILD_DROP_VERSION" \
  --build-arg BUILD_GIT_REF="$BUILD_GIT_REF" \
  --build-arg AUDIT_DATE="$(date -u +%F)" \
  .

# Fastest gzip level, on every core when pigz is there: tarfile's own gzip
# runs at level 9 on one core and took as long as the build itself. The size
# barely matters, docker load unpacks it all again anyway.
if command -v pigz >/dev/null; then
  COMPRESS=(pigz -1)
else
  COMPRESS=(gzip -1)
fi

# `docker save` can't write an unqualified name: podman (which `docker` is in
# the dev container) qualifies it with a registry (localhost/drop:custom).
# Rewrite the name in the archive's manifest.json and repositories files.
docker save "$IMAGE_TAG" | python3 -c '
import io, json, sys, tarfile

name, tag = sys.argv[1].split(":")
src = tarfile.open(fileobj=sys.stdin.buffer, mode="r|")
# Written uncompressed; the compressor below gzips the stream.
dst = tarfile.open(fileobj=sys.stdout.buffer, mode="w|", bufsize=1 << 20)
for member in src:
    data = src.extractfile(member) if member.isfile() else None
    if member.name == "manifest.json":
        manifest = json.load(data)
        for image in manifest:
            image["RepoTags"] = [f"{name}:{tag}"]
        data = json.dumps(manifest).encode()
    elif member.name == "repositories":
        image_id = next(iter(next(iter(json.load(data).values())).values()))
        data = json.dumps({name: {tag: image_id}}).encode()
    else:
        dst.addfile(member, data)
        continue
    member.size = len(data)
    dst.addfile(member, io.BytesIO(data))
dst.close()
' "$IMAGE_TAG" | "${COMPRESS[@]}" > "$OUTPUT_FILE"

echo "Built $IMAGE_TAG and saved it to $(pwd)/$OUTPUT_FILE"
