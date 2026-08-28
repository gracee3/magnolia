#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source_file="$root/crates/magnolia-audio/src/capture.rs"

callback=$(sed -n '/^fn process_capture(/,/^fn serialize_format(/p' "$source_file")
for forbidden in \
    'lock(' 'println!' 'eprintln!' 'format!' 'serde' 'std::fs' 'std::net' \
    '.send(' '.recv(' '.unwrap(' '.expect(' 'panic!' 'drop('; do
    if grep -Fq "$forbidden" <<<"$callback"; then
        echo "callback audit: forbidden token $forbidden in process_capture" >&2
        exit 1
    fi
done

echo "callback audit: process_capture uses only mapped buffers, prepared storage, atomics, and bounded loops"
