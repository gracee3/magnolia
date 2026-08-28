#!/usr/bin/env bash
set -euo pipefail

if ! command -v rg >/dev/null 2>&1; then
  echo "required verification dependency is missing: rg" >&2
  exit 1
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

./scripts/check.sh
./scripts/check-phase-2.sh

git diff --check HEAD
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
