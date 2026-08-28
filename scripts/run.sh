#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
for tool in cargo trunk; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "required Magnolia launch dependency is missing: $tool" >&2
    exit 1
  fi
done

export CARGO_TARGET_DIR="$repo_root/target/browser"
export NO_COLOR=false
assets="$repo_root/target/magnolia-studio-web-dist"

cd "$repo_root/crates/magnolia-studio-web"
trunk build --release --locked --dist "$assets"

cd "$repo_root"
cargo run --locked --package magnolia-desktop -- \
  --assets "$assets" "$@"
