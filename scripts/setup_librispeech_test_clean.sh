#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
data_root="${repo_root}/tools/LibriSpeech"
archive="${data_root}/test-clean.tar.gz"
url="https://www.openslr.org/resources/12/test-clean.tar.gz"

mkdir -p "$data_root"
if [[ ! -d "${data_root}/LibriSpeech/test-clean" ]]; then
    curl --fail --location --retry 3 --output "$archive" "$url"
    tar -xzf "$archive" -C "$data_root"
    rm -f "$archive"
fi
test -d "${data_root}/LibriSpeech/test-clean"
echo "LibriSpeech test-clean is ready under ${data_root}/LibriSpeech/test-clean"
