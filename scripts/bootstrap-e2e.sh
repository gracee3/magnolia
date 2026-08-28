#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
node_version=24.20.0
node_archive="node-v${node_version}-linux-x64.tar.xz"
node_checksum=2f2c0da162318f0de47665410c7c8c2ed3d36c8f3105de4bbc61176c70a7cbf2
local_node="$repo_root/.tools/node-v${node_version}-linux-x64"

if command -v node >/dev/null 2>&1 && command -v npm >/dev/null 2>&1; then
  node_bin=$(command -v node)
  npm_bin=$(command -v npm)
else
  for tool in curl sha256sum tar; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      echo "required E2E bootstrap dependency is missing: $tool" >&2
      exit 1
    fi
  done
  if [[ ! -x "$local_node/bin/node" ]]; then
    mkdir -p "$repo_root/.tools"
    download_dir=$(mktemp -d)
    archive="$download_dir/$node_archive"
    curl --fail --silent --show-error --location \
      "https://nodejs.org/dist/v${node_version}/${node_archive}" \
      --output "$archive"
    printf '%s  %s\n' "$node_checksum" "$archive" | sha256sum --check --status
    tar -xJf "$archive" -C "$repo_root/.tools"
  fi
  node_bin="$local_node/bin/node"
  npm_bin="$local_node/bin/npm"
fi

node_major=$($node_bin --version | sed -E 's/^v([0-9]+).*/\1/')
if [[ "$node_major" -lt 24 ]]; then
  echo "Node.js 24 or newer is required for browser E2E; found $($node_bin --version)" >&2
  exit 1
fi

export PATH="$(dirname "$node_bin"):$PATH"
cd "$repo_root/tests/e2e"
"$npm_bin" ci
printf 'E2E dependencies installed with Node %s\n' "$($node_bin --version)"
