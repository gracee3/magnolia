#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
desktop_manifest="$repo_root/apps/magnolia-desktop/Cargo.toml"
web_root="$repo_root/crates/magnolia-studio-web"
assets="$repo_root/target/magnolia-studio-web-dist"

for tool in cargo rg rustup trunk; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "required browser verification dependency is missing: $tool" >&2
    exit 1
  fi
done

if ! rustup target list --installed | rg --quiet '^wasm32-unknown-unknown$'; then
  echo "required Rust target is missing: wasm32-unknown-unknown" >&2
  exit 1
fi

export CARGO_TARGET_DIR="$repo_root/target/browser"
export NO_COLOR=false

cargo fmt --manifest-path "$desktop_manifest" --all --check
cargo check --locked --manifest-path "$desktop_manifest" --workspace --all-targets
cargo test --locked --manifest-path "$desktop_manifest" --package magnolia-desktop --all-targets
cargo clippy --locked --manifest-path "$desktop_manifest" --workspace --all-targets -- -D warnings
cargo check --locked --manifest-path "$desktop_manifest" \
  --package magnolia-studio-web --target wasm32-unknown-unknown

cd "$web_root"
trunk build --release --locked --dist "$assets"

cd "$repo_root"
unset NO_COLOR
if command -v node >/dev/null 2>&1 && command -v npm >/dev/null 2>&1; then
  node_bin=$(command -v node)
  npm_bin=$(command -v npm)
else
  local_node="$repo_root/.tools/node-v24.20.0-linux-x64"
  if [[ ! -x "$local_node/bin/node" || ! -x "$local_node/bin/npm" ]]; then
    echo "Node.js and npm are required for Chromium E2E; run ./scripts/bootstrap-e2e.sh" >&2
    exit 1
  fi
  node_bin="$local_node/bin/node"
  npm_bin="$local_node/bin/npm"
  export PATH="$local_node/bin:$PATH"
fi

node_major=$($node_bin --version | sed -E 's/^v([0-9]+).*/\1/')
if [[ "$node_major" -lt 24 ]]; then
  echo "Node.js 24 or newer is required for Chromium E2E; found $($node_bin --version)" >&2
  exit 1
fi

if [[ ! -d "$repo_root/tests/e2e/node_modules/@playwright/test" ]]; then
  echo "Playwright dependencies are missing; run ./scripts/bootstrap-e2e.sh" >&2
  exit 1
fi

if [[ -z "${MAGNOLIA_CHROMIUM:-}" ]]; then
  for candidate in chromium chromium-browser google-chrome google-chrome-stable; do
    if command -v "$candidate" >/dev/null 2>&1; then
      export MAGNOLIA_CHROMIUM
      MAGNOLIA_CHROMIUM=$(command -v "$candidate")
      break
    fi
  done
fi
if [[ -z "${MAGNOLIA_CHROMIUM:-}" ]]; then
  playwright_chromium=$(
    cd "$repo_root/tests/e2e"
    "$node_bin" --input-type=module --eval \
      'import { chromium } from "@playwright/test"; console.log(chromium.executablePath())'
  )
  if [[ ! -x "$playwright_chromium" ]]; then
    echo "Chromium is missing; set MAGNOLIA_CHROMIUM or run 'npx playwright install chromium' in tests/e2e" >&2
    exit 1
  fi
fi

export MAGNOLIA_DESKTOP_BIN="$repo_root/target/browser/debug/magnolia-desktop"
export MAGNOLIA_WEB_ASSETS="$assets"
"$npm_bin" test --prefix "$repo_root/tests/e2e"
