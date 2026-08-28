#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

./scripts/check.sh

cargo test --locked --package magnolia-runtime \
  --test foundation_round_trip \
  portable_foundation_round_trip_preserves_last_good_and_ignores_stale_results \
  -- --exact

git diff --check

for file in README.md AGENTS.md $(find docs -type f -name '*.md' -print); do
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
  done < <(rg -o '\[[^]]*\]\([^)]+\)' "$file" || true)
done
