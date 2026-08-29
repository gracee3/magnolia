#!/usr/bin/env bash
set -euo pipefail

for tool in cp curl cut find jq mktemp mv rg sha256sum stat tar; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "required acquisition dependency is missing: $tool" >&2
    exit 1
  }
done

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
lock=${MAGNOLIA_SHERPA_LOCK:-$repo_root/configs/sherpa-1.13.4.lock.json}
model_root=${MAGNOLIA_MODEL_ROOT:-${XDG_DATA_HOME:-$HOME/.local/share}/magnolia/models}

[[ $(jq -r '.schema_major' "$lock") == 1 ]] || {
  echo "unsupported Sherpa acquisition lock schema" >&2
  exit 1
}
[[ $(jq -r '.adapter_version' "$lock") == 1.13.4 ]] || {
  echo "Sherpa adapter lock must be exactly 1.13.4" >&2
  exit 1
}
[[ $(jq -r '.model_asset_id' "$lock") == 191971614 ]] || {
  echo "unexpected model asset id" >&2
  exit 1
}

# A missing digest is an intentional hard stop. No network request or directory
# creation occurs before both upstream-published hashes are present.
for key in model native_library; do
  digest=$(jq -r ".${key}.sha256 // empty" "$lock")
  if [[ ! $digest =~ ^[0-9a-fA-F]{64}$ ]]; then
    echo "$key artifact lacks an authoritative SHA-256; acquisition refused" >&2
    exit 2
  fi
done

mkdir -p "$model_root"
stage=$(mktemp -d "$model_root/.phase6-acquire.XXXXXX")
cleanup() {
  rm -rf -- "$stage"
}
trap cleanup EXIT

acquire_artifact() {
  local key=$1
  local archive=$stage/$key.tar.bz2.partial
  local extract=$stage/$key
  local url expected_bytes expected_hash actual_bytes actual_hash
  url=$(jq -r ".${key}.source_url" "$lock")
  expected_bytes=$(jq -r ".${key}.expected_bytes" "$lock")
  expected_hash=$(jq -r ".${key}.sha256" "$lock")
  curl --fail --location --continue-at - --output "$archive" "$url"
  actual_bytes=$(stat --format=%s "$archive")
  [[ $actual_bytes == "$expected_bytes" ]] || {
    echo "$key archive size mismatch" >&2
    return 1
  }
  actual_hash=$(sha256sum "$archive" | cut -d' ' -f1)
  [[ $actual_hash == "$expected_hash" ]] || {
    echo "$key archive hash mismatch" >&2
    return 1
  }
  if tar -tjf "$archive" | rg --quiet '(^/|(^|/)\.\.(/|$))'; then
    echo "$key archive contains an unsafe path" >&2
    return 1
  fi
  mkdir "$extract"
  tar -xjf "$archive" -C "$extract" --no-same-owner --no-same-permissions
}

acquire_artifact model
acquire_artifact native_library

for filename in encoder-epoch-99-avg-1.onnx decoder-epoch-99-avg-1.onnx joiner-epoch-99-avg-1.onnx tokens.txt; do
  expected=$(jq -r --arg file "$filename" '.model.extracted_sha256[$file] // empty' "$lock")
  [[ $expected =~ ^[0-9a-fA-F]{64}$ ]] || {
    echo "model lacks an authoritative extracted hash for $filename" >&2
    exit 2
  }
  mapfile -t matches < <(find "$stage/model" -type f -name "$filename" -print)
  [[ ${#matches[@]} == 1 ]] || {
    echo "model archive does not contain exactly one $filename" >&2
    exit 1
  }
  [[ $(sha256sum "${matches[0]}" | cut -d' ' -f1) == "$expected" ]] || {
    echo "extracted model hash mismatch for $filename" >&2
    exit 1
  }
done

destination=$model_root/sherpa-onnx-1.13.4-zipformer-en
[[ ! -e $destination ]] || {
  echo "model destination already exists: $destination" >&2
  exit 1
}
cp "$lock" "$stage/provenance-lock.json"
mv "$stage" "$destination"
trap - EXIT
echo "validated Sherpa artifacts published at $destination"
