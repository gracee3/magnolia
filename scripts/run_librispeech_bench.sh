#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
corpus="${repo_root}/tools/LibriSpeech/LibriSpeech/test-clean"
limit="${MAGNOLIA_BENCH_LIMIT:-10}"
test -d "$corpus" || { echo "Run scripts/setup_librispeech_test_clean.sh first" >&2; exit 1; }

count=0
while IFS= read -r flac; do
    id="$(basename "$flac" .flac)"
    book_dir="$(dirname "$flac")"
    transcript="${book_dir}/$(basename "$(dirname "$book_dir")")-$(basename "$book_dir").trans.txt"
    reference="$(awk -v id="$id" '$1 == id { sub(/^[^ ]+ /, ""); print; exit }' "$transcript")"
    [[ -n "$reference" ]] || continue
    wav="$(mktemp --suffix=.wav)"
    trap 'rm -f "$wav"' EXIT
    ffmpeg -loglevel error -y -i "$flac" -ar 16000 -ac 1 "$wav"
    (cd "$repo_root" && cargo run --release -q -p stt_bench -- "$wav" "$reference")
    rm -f "$wav"
    trap - EXIT
    count=$((count + 1))
    [[ "$count" -ge "$limit" ]] && break
done < <(find "$corpus" -type f -name '*.flac' | sort)
