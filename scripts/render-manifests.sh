#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIG="${1:-$ROOT/config/server.toml}"
OUT="${2:-$ROOT/k8s/generated/manifests.yaml}"
cd "$ROOT"

mkdir -p "$(dirname "$OUT")"

docker build -f docker/Dockerfile.rust -t minecraft-k8s-tools:local "$ROOT"
docker run --rm \
  -v "$ROOT:/src:ro" \
  -v "$(dirname "$OUT"):/out" \
  -w /src \
  minecraft-k8s-tools:local \
  render --config "/src/${CONFIG#"$ROOT/"}" --output "/out/$(basename "$OUT")"

echo "Rendered manifests to $OUT"
