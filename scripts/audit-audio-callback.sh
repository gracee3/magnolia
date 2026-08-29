#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
audit_callback() {
    local source_file=$1 start=$2 end=$3 callback forbidden
    callback=$(sed -n "/^fn $start(/,/^fn $end(/p" "$source_file")
    for forbidden in \
        'lock(' 'println!' 'eprintln!' 'format!' 'serde' 'std::fs' 'std::net' \
        '.send(' '.recv(' '.unwrap(' '.expect(' 'panic!' 'drop('; do
        if grep -Fq "$forbidden" <<<"$callback"; then
            echo "callback audit: forbidden token $forbidden in $start" >&2
            exit 1
        fi
    done
}

audit_callback "$root/crates/magnolia-audio/src/capture.rs" process_capture serialize_format
audit_callback "$root/crates/magnolia-audio/src/output.rs" process_output serialize_output_format
echo "callback audit: capture and output callbacks use only mapped buffers, prepared storage, atomics, and bounded loops"
