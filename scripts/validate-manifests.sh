#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="${1:-$ROOT/k8s/manifests.yaml}"
cd "$ROOT"

docker build -f docker/Dockerfile.rust -t minecraft-k8s-tools:local "$ROOT"
docker run --rm -v "$ROOT:/src:ro" -w /src minecraft-k8s-tools:local \
  check-manifests --path "/src/${MANIFEST#"$ROOT/"}"
