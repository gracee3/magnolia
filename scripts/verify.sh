#!/usr/bin/env bash
set -euo pipefail

for tool in cargo git jq rg rustup trunk; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "required verification dependency is missing: $tool" >&2
    exit 1
  fi
done

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

./scripts/check.sh
./scripts/check-browser.sh

if [[ -n "$(git status --porcelain)" ]]; then
  echo "verification requires a clean exact-SHA worktree" >&2
  exit 1
fi

verification_base=807b032a12e737cdeca5137004697ea0a2a97537
git merge-base --is-ancestor "$verification_base" HEAD
git diff --check "$verification_base"...HEAD
git diff --check
git diff --cached --check

expected_members=$'magnolia-application\nmagnolia-audio\nmagnolia-client\nmagnolia-desktop\nmagnolia-domain\nmagnolia-observe\nmagnolia-protocol\nmagnolia-runtime\nmagnolia-studio-web'
actual_members=$(cargo metadata --locked --no-deps --format-version 1 | jq -r \
  '.workspace_members as $members | [.packages[] | select(.id as $id | $members | index($id)) | .name] | sort | .[]')
if [[ "$actual_members" != "$expected_members" ]]; then
  echo "workspace member audit failed" >&2
  diff -u <(printf '%s\n' "$expected_members") <(printf '%s\n' "$actual_members") || true
  exit 1
fi

if find .github/workflows -type f -print -quit 2>/dev/null | rg --quiet .; then
  echo "repository workflow audit failed" >&2
  exit 1
fi

if git ls-files | rg --quiet '\.(so|dylib|dll|exe)$'; then
  echo "tracked binary audit failed" >&2
  exit 1
fi

active_paths=(Cargo.toml .cargo apps crates tests)
legacy_pattern='nannou|magnolia[_-](core|signals|module-api|plugin-abi|plugin-helper|ui)|libloading|ed25519-dalek|seccompiler|layout\.toml|run-phase-2|check-phase-2|libhello_plugin'
if rg -n -i "$legacy_pattern" "${active_paths[@]}"; then
  echo "legacy source or dependency residue audit failed" >&2
  exit 1
fi
if rg -n -i 'run-phase-2|check-phase-2|libhello_plugin' scripts --glob '!verify.sh'; then
  echo "legacy script residue audit failed" >&2
  exit 1
fi

cargo tree --locked --workspace --prefix none | \
  rg -i '^(nannou|wgpu|ed25519-dalek|seccompiler|notify) ' && {
    echo "legacy dependency audit failed" >&2
    exit 1
  }

if cargo metadata --locked --no-deps --format-version 1 | jq -r \
  '.packages[] | select(.source == null) | .dependencies[].name' | \
  rg -i '^(libloading|ed25519-dalek|seccompiler|notify)$'; then
  echo "direct legacy dependency audit failed" >&2
  exit 1
fi

for file in README.md AGENTS.md $(find docs -type f -name '*.md' -print); do
  link_matches=$(rg -o '\[[^]]*\]\([^)]+\)' "$file") || {
    rg_status=$?
    if [[ $rg_status -ne 1 ]]; then
      echo "Markdown-link scan failed for $file" >&2
      exit "$rg_status"
    fi
  }
  while IFS= read -r raw_link; do
    link=${raw_link#*](}
    link=${link%)}
    path=${link%%#*}
    [[ -z "$path" ]] && continue
    case "$path" in
      http://*|https://*|mailto:*) continue ;;
    esac
    target=$(dirname "$file")/$path
    if [[ ! -e "$target" ]]; then
      echo "broken internal Markdown link: $file -> $link" >&2
      exit 1
    fi
  done <<< "$link_matches"
done
