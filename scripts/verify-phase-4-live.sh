#!/usr/bin/env bash
set -euo pipefail

for tool in cargo git pactl pw-dump pw-loopback rg trunk wpctl; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "required Phase 4 live dependency is missing: $tool" >&2
    exit 1
  fi
done

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

if [[ ${MAGNOLIA_ALLOW_DIRTY_LIVE:-0} != 1 && -n $(git status --porcelain) ]]; then
  echo "Phase 4 live verification requires a clean exact-SHA worktree" >&2
  exit 1
fi

source_name=${MAGNOLIA_PHASE4_SOURCE:-$(pactl get-default-source)}
capture_seconds=${MAGNOLIA_PHASE4_CAPTURE_SECONDS:-5}
soak_seconds=${MAGNOLIA_PHASE4_SOAK_SECONDS:-1800}
cycles=${MAGNOLIA_PHASE4_CYCLES:-20}
probe_root=$(mktemp -d)
loop_pid=
recovery_pid=
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
  echo "Phase 4 browser dependencies are missing; run ./scripts/bootstrap-e2e.sh" >&2
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
  echo "Chromium is unavailable for the Phase 4 reload gate" >&2
  exit 1
fi

cleanup() {
  local status=$?
  trap - EXIT INT TERM
  if [[ -n $recovery_pid ]]; then
    kill "$recovery_pid" 2>/dev/null || true
    wait "$recovery_pid" 2>/dev/null || true
  fi
  if [[ -n $loop_pid ]]; then
    kill "$loop_pid" 2>/dev/null || true
    wait "$loop_pid" 2>/dev/null || true
  fi
  if pw-dump | rg -q 'magnolia_phase4_(source|sink)'; then
    echo "temporary Phase 4 PipeWire nodes survived cleanup" >&2
    status=1
  fi
  if [[ $(pactl get-default-source) != "$original_source" \
      || $(pactl get-default-sink) != "$original_sink" \
      || $(wpctl get-volume @DEFAULT_AUDIO_SOURCE@) != "$original_source_volume" \
      || $(wpctl get-volume @DEFAULT_AUDIO_SINK@) != "$original_sink_volume" ]]; then
    echo "user-session default, mute, or volume state changed during the live gate" >&2
    status=1
  fi
  rm -r "$probe_root"
  exit "$status"
}
trap cleanup EXIT INT TERM

start_loopback() {
  pw-loopback \
    --name magnolia-phase4-loop \
    --capture-props='node.name=magnolia_phase4_sink media.class=Audio/Sink' \
    --playback-props='node.name=magnolia_phase4_source media.class=Audio/Source' \
    >"$probe_root/loopback.log" 2>&1 &
  loop_pid=$!
}

wait_for_log() {
  local pattern=$1
  for _ in $(seq 1 80); do
    if rg -q "$pattern" "$probe_root/recovery.log" 2>/dev/null; then
      return 0
    fi
    sleep 0.25
  done
  echo "timed out waiting for recovery evidence: $pattern" >&2
  return 1
}

./scripts/audit-audio-callback.sh
cargo build --locked -p magnolia-audio --example capture_probe --example monitor_probe
cargo build --locked -p magnolia-runtime --example recovery_probe

cargo run --locked -p magnolia-audio --example capture_probe -- \
  "$source_name" "$capture_seconds"

if (( soak_seconds > 0 )); then
  cargo run --locked -p magnolia-audio --example capture_probe -- \
    "$source_name" "$soak_seconds"
fi

for _ in $(seq 1 "$cycles"); do
  cargo run --quiet --locked -p magnolia-audio --example capture_probe -- \
    "$source_name" 1
done

cargo run --locked -p magnolia-audio --example monitor_probe -- "$source_name"

export CARGO_TARGET_DIR="$repo_root/target/browser"
cargo build --locked -p magnolia-desktop
(
  cd "$repo_root/crates/magnolia-studio-web"
  NO_COLOR=false trunk build --release --locked --dist "$repo_root/target/magnolia-studio-web-dist"
)
MAGNOLIA_CHROMIUM="$chromium" \
MAGNOLIA_DESKTOP_BIN="$repo_root/target/browser/debug/magnolia-desktop" \
MAGNOLIA_WEB_ASSETS="$repo_root/target/magnolia-studio-web-dist" \
  "$node_bin" "$repo_root/tests/e2e/live-audio.mjs"
unset CARGO_TARGET_DIR

start_loopback
sleep 1
cargo run --locked -p magnolia-runtime --example recovery_probe -- \
  magnolia_phase4_source >"$probe_root/recovery.log" 2>&1 &
recovery_pid=$!
wait_for_log READY
kill "$loop_pid"
wait "$loop_pid" || true
loop_pid=
wait_for_log DEGRADED
start_loopback
wait "$recovery_pid"
recovery_pid=
rg 'READY|DEGRADED|RECOVERED|CONTROLS' "$probe_root/recovery.log"

echo "Phase 4 live verification passed; no audio was recorded and session controls were unchanged"
