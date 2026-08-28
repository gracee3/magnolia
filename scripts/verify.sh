#!/usr/bin/env bash
set -euo pipefail

if ! command -v rg >/dev/null 2>&1; then
  echo "required verification dependency is missing: rg" >&2
  exit 1
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

./scripts/check.sh

cargo test --locked --package magnolia-runtime \
  --test foundation_round_trip \
  portable_foundation_round_trip_preserves_last_good_and_ignores_stale_results \
  -- --exact

verification_suite=${MAGNOLIA_VERIFY_SUITE:-all}
case "$verification_suite" in
  all) ./scripts/check-phase-2.sh ;;
  foundation) ;;
  *)
    echo "unknown Magnolia verification suite: $verification_suite" >&2
    exit 1
    ;;
esac

verification_base=${MAGNOLIA_VERIFY_BASE:-origin/main}
if git rev-parse --verify "$verification_base^{commit}" >/dev/null 2>&1; then
  git diff --check "$verification_base"...HEAD
  changed_paths=$(
    {
      git diff --name-only "$verification_base"...HEAD
      git diff --name-only
      git diff --cached --name-only
      git ls-files --others --exclude-standard
    } | sort -u
  )
  while IFS= read -r changed_path; do
    [[ -z "$changed_path" ]] && continue
    case "$changed_path" in
      .github/workflows/*|.gitignore|AGENTS.md|Cargo.toml|README.md|docs/*|scripts/*|tests/*|apps/magnolia-desktop/*|crates/magnolia-application/*|crates/magnolia-client/*|crates/magnolia-domain/*|crates/magnolia-protocol/*|crates/magnolia-studio-web/*) ;;
      *)
        echo "Phase 2 changed-path audit rejected: $changed_path" >&2
        exit 1
        ;;
    esac
  done <<< "$changed_paths"
fi
git diff --check
git diff --cached --check

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
