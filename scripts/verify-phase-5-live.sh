#!/usr/bin/env bash
set -euo pipefail

for tool in cargo git pactl pw-dump rg trunk wpctl; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "required Phase 5 live dependency is missing: $tool" >&2
    exit 1
  fi
done

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

if [[ ${MAGNOLIA_ALLOW_DIRTY_LIVE:-0} != 1 && -n $(git status --porcelain) ]]; then
  echo "Phase 5 live verification requires a clean exact-SHA worktree" >&2
  exit 1
fi

source_name=${MAGNOLIA_PHASE5_SOURCE:-$(pactl get-default-source)}
soak_seconds=${MAGNOLIA_PHASE5_SOAK_SECONDS:-1800}
probe_root=$(mktemp -d)
original_source=$(pactl get-default-source)
original_sink=$(pactl get-default-sink)
original_source_volume=$(wpctl get-volume @DEFAULT_AUDIO_SOURCE@)
original_sink_volume=$(wpctl get-volume @DEFAULT_AUDIO_SINK@)

if command -v node >/dev/null 2>&1; then
  node_bin=$(command -v node)
else
  node_bin="$repo_root/.tools/node-v24.20.0-linux-x64/bin/node"
fi
if [[ ! -x $node_bin || ! -d $repo_root/tests/e2e/node_modules/@playwright/test ]]; then
  echo "Phase 5 browser dependencies are missing; run ./scripts/bootstrap-e2e.sh" >&2
  exit 1
fi
chromium=${MAGNOLIA_CHROMIUM:-}
if [[ -z $chromium ]]; then
  for candidate in chromium chromium-browser google-chrome google-chrome-stable; do
    if command -v "$candidate" >/dev/null 2>&1; then
      chromium=$(command -v "$candidate")
      break
    fi
  done
fi
if [[ ! -x $chromium ]]; then
  echo "Chromium is unavailable for the Phase 5 observation gate" >&2
  exit 1
fi

cleanup() {
  local status=$?
  trap - EXIT INT TERM
  if pw-dump | rg -q 'magnolia-(capture|output|phase5)'; then
    echo "Magnolia PipeWire nodes survived Phase 5 teardown" >&2
    status=1
  fi
  if [[ $(pactl get-default-source) != "$original_source" \
      || $(pactl get-default-sink) != "$original_sink" \
      || $(wpctl get-volume @DEFAULT_AUDIO_SOURCE@) != "$original_source_volume" \
      || $(wpctl get-volume @DEFAULT_AUDIO_SINK@) != "$original_sink_volume" ]]; then
    echo "user-session default, mute, or volume state changed during the Phase 5 gate" >&2
    status=1
  fi
  rm -r "$probe_root"
  exit "$status"
}
trap cleanup EXIT INT TERM

./scripts/audit-audio-callback.sh
cargo test --locked -p magnolia-observe
cargo build --locked -p magnolia-observe --examples

# This opens the selected input for analysis but never gives microphone samples
# to the recording worker. The soak default is the required thirty minutes.
cargo run --locked -p magnolia-observe --example live_observation_probe -- \
  "$source_name" "$soak_seconds"

# Durable recording/replay evidence uses generated seeded PCM only.
cargo run --locked -p magnolia-observe --example record_replay_probe -- \
  "$probe_root/recordings"

export CARGO_TARGET_DIR="$repo_root/target/browser"
cargo build --locked -p magnolia-desktop
(
  cd "$repo_root/crates/magnolia-studio-web"
  NO_COLOR=false trunk build --release --locked --dist "$repo_root/target/magnolia-studio-web-dist"
)
MAGNOLIA_CHROMIUM="$chromium" \
MAGNOLIA_DESKTOP_BIN="$repo_root/target/browser/debug/magnolia-desktop" \
MAGNOLIA_WEB_ASSETS="$repo_root/target/magnolia-studio-web-dist" \
  "$node_bin" "$repo_root/tests/e2e/live-observation.mjs"
unset CARGO_TARGET_DIR

echo "Phase 5 live observation/record/replay verification passed; physical microphone audio was not recorded"
