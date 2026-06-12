#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

docker build -f docker/Dockerfile.rust-test -t minecraft-k8s-test:local .
docker run --rm --security-opt seccomp=unconfined -v "$ROOT:/src" -w /src minecraft-k8s-test:local \
  "cargo test --all-features --workspace && cargo tarpaulin --workspace --out Xml --output-dir /tmp/coverage --fail-under 100 --timeout 300 --exclude-files '*/main.rs' '*/tests/*'"
