#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ -z "${GH_TOKEN:-}" ]]; then
  echo "GH_TOKEN is required to create the GitHub repository via API." >&2
  echo "Create a token at https://github.com/settings/tokens (repo scope)." >&2
  echo "Then run: GH_TOKEN=<token> $0" >&2
  exit 1
fi

OWNER="brianlechthaler"
REPO="minecraft-k8s"

if ! curl -fsS "https://api.github.com/repos/${OWNER}/${REPO}" >/dev/null 2>&1; then
  curl -fsS -X POST \
    -H "Authorization: Bearer ${GH_TOKEN}" \
    -H "Accept: application/vnd.github+json" \
    https://api.github.com/user/repos \
    -d "{\"name\":\"${REPO}\",\"description\":\"Modded Minecraft server on Kubernetes with Rust tooling and Docker\",\"private\":false}" >/dev/null
  echo "Created https://github.com/${OWNER}/${REPO}"
else
  echo "Repository already exists"
fi

git remote set-url origin "git@github.com:${OWNER}/${REPO}.git"
GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=accept-new" git push -u origin main
echo "Published to https://github.com/${OWNER}/${REPO}"
