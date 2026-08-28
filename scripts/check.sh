#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

packages=(
  magnolia-domain
  magnolia-protocol
  magnolia-client
  magnolia-application
  magnolia-runtime
)

package_args=()
for package in "${packages[@]}"; do
  package_args+=(--package "$package")
done

cargo fmt --all --check
cargo check --locked --target wasm32-unknown-unknown \
  --package magnolia-domain \
  --package magnolia-protocol \
  --package magnolia-client
cargo check --locked "${package_args[@]}" --all-targets
cargo test --locked "${package_args[@]}"
cargo clippy --locked "${package_args[@]}" --all-targets -- -D warnings
