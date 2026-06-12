#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TAG="${1:-latest}"

docker build -f docker/Dockerfile.rust -t "ghcr.io/brianlechthaler/minecraft-k8s-tools:${TAG}" .
docker build -f docker/Dockerfile.minecraft -t "ghcr.io/brianlechthaler/minecraft-k8s-server:${TAG}" .
