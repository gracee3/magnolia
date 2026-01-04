#!/usr/bin/env bash
set -euo pipefail

# =========================
# Config (override via env)
# =========================
ROUNDS="${ROUNDS:-3}"
OUT_DIR="${OUT_DIR:-/tmp/magnolia_stt_suite}"
VARIANTS="${VARIANTS:-base}"
MODEL_DIR="${MODEL_DIR:-/home/emmy/git/trt-asr-engine/models/parakeet-tdt-0.6b-v3}"
MAGNOLIA_DIR="${MAGNOLIA_DIR:-/home/emmy/git/magnolia}"
TRT_ENGINE_LIB="${TRT_ENGINE_LIB:-/home/emmy/git/trt-asr-engine/cpp/build}"
TRT_CLI_BIN="${TRT_CLI_BIN:-/home/emmy/git/trt-asr-engine/rust/target/debug/cli}"

# SOF device: note this endpoint is fixed 2ch for DEV=7 on your system.
SOF_DEV="${SOF_DEV:-hw:CARD=sofhdadsp,DEV=7}"
SOF_RATE="${SOF_RATE:-16000}"
SOF_CH="${SOF_CH:-2}"          # DEV=7 is 2ch; do not set to 1 for hw:...
SOF_FMT="${SOF_FMT:-S16_LE}"
SOF_SECS="${SOF_SECS:-10}"     # quick capture to validate input chain

# Feature replay input (deterministic)
FEATURE_JSON="${FEATURE_JSON:-/home/emmy/git/trt-asr-engine/debug_artifacts/tap_FEATURES.json}"
REPLAY_REQUIRE_RUN_TAP="${REPLAY_REQUIRE_RUN_TAP:-0}"

# Toggles
DO_CAPTURE="${DO_CAPTURE:-1}"      # 1 = record a short WAV from SOF
DO_MAGNOLIA="${DO_MAGNOLIA:-1}"    # 1 = run magnolia daemon briefly
DO_REPLAY="${DO_REPLAY:-1}"        # 1 = run cli replay from FEATURE_JSON
DO_ANALYZE="${DO_ANALYZE:-0}"      # 1 = run analyze_tap.py if present

# Magnolia run duration (seconds)
MAGNOLIA_TIMEOUT="${MAGNOLIA_TIMEOUT:-15}"

# Debug knobs
PARAKEET_DEBUG_SYNC="${PARAKEET_DEBUG_SYNC:-1}"
PARAKEET_DEBUG_SYNC_MEMCPY="${PARAKEET_DEBUG_SYNC_MEMCPY:-1}"
PARAKEET_DEBUG_BLANK_SCAN="${PARAKEET_DEBUG_BLANK_SCAN:-1}"
PARAKEET_DEBUG_EMIT_TOKENS="${PARAKEET_DEBUG_EMIT_TOKENS:-1}"
PARAKEET_DISABLE_PUNCT_SUPPRESSION="${PARAKEET_DISABLE_PUNCT_SUPPRESSION:-0}" # try 0/1
PARAKEET_Y0_OVERRIDE="${PARAKEET_Y0_OVERRIDE:-64}" # 64 = tok_lang in your logs
PARAKEET_DISABLE_CACHE="${PARAKEET_DISABLE_CACHE:-0}"

# Logging
RUST_LOG_MAGNOLIA="${RUST_LOG_MAGNOLIA:-parakeet_stt=debug,daemon=info}"

# =========================
# Helpers
# =========================
ts() { date +"%Y%m%d_%H%M%S"; }

say() { echo -e "\n==> $*"; }

BEEP_DEV="${BEEP_DEV:-default}"
BEEP_ENABLED="${BEEP_ENABLED:-1}"
BEEP_WAV=""

init_beep_wav() {
  if [[ -n "$BEEP_WAV" ]]; then
    return 0
  fi
  if ! command -v aplay >/dev/null 2>&1; then
    return 1
  fi
  if ! command -v python3 >/dev/null 2>&1; then
    return 1
  fi
  BEEP_WAV="$(mktemp /tmp/magnolia_beep_XXXXXX.wav)"
  python3 - "$BEEP_WAV" <<'PY'
import math
import struct
import sys
import wave

path = sys.argv[1]
rate = 44100
freq = 880.0
duration = 0.2
amp = 0.3
n = int(rate * duration)

with wave.open(path, "wb") as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(rate)
    frames = bytearray()
    for i in range(n):
        v = int(amp * 32767 * math.sin(2.0 * math.pi * freq * i / rate))
        frames += struct.pack("<h", v)
    w.writeframes(frames)
PY
  return 0
}

cleanup_beep_wav() {
  if [[ -n "$BEEP_WAV" && -f "$BEEP_WAV" ]]; then
    rm -f "$BEEP_WAV"
  fi
}

trap cleanup_beep_wav EXIT

beep() {
  if [[ "$BEEP_ENABLED" != "1" ]]; then
    return 0
  fi
  if init_beep_wav; then
    aplay -q -D "$BEEP_DEV" "$BEEP_WAV" >/dev/null 2>&1 || true
  else
    printf '\a'
  fi
}

beep_times() {
  local count="$1"
  local i=0
  while [[ $i -lt $count ]]; do
    beep
    sleep 0.2
    i=$((i + 1))
  done
}

mk_run_dir() {
  local run_id="run_$(ts)_pid$$"
  local rd="$OUT_DIR/$run_id"
  mkdir -p "$rd"
  echo "$rd"
}

summarize_log() {
  local logfile="$1"
  echo "---- Summary: $(basename "$logfile") ----"
  # NaN guard / encoder / joint stats
  rg -n "NAN_GUARD|enc_out_stats|joint_out_stats|joint_in_stats" "$logfile" 2>/dev/null | tail -50 || true
  # Blank scan stats
  rg -n "blank_scan" "$logfile" 2>/dev/null | tail -50 || true
  # Token emission summary / forced time advance
  rg -n "emit_summary|emit_token|forced time_idx" "$logfile" 2>/dev/null | tail -60 || true
  echo "----------------------------------------"
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "ERROR: missing required command: $1" >&2
    exit 1
  }
}

usage() {
  cat <<'EOF'
Usage: run_stt_debug_suite.sh [options]

Options:
  --rounds N         Number of rounds per variant (default: 3)
  --duration S       Capture duration in seconds (default: 10)
  --out-dir DIR      Output directory (default: /tmp/magnolia_stt_suite)
  --variants "..."   Space-separated variants: base nopunct nocache nocache_nopunct
  --no-capture       Skip SOF capture
  --no-magnolia      Skip Magnolia run
  --no-replay        Skip feature replay
  --no-analyze       Skip analyze_tap.py
  -h, --help         Show this help

Environment overrides:
  SOF_DEV, SOF_RATE, SOF_CH, SOF_FMT, MODEL_DIR, MAGNOLIA_DIR, TRT_ENGINE_LIB,
  TRT_CLI_BIN, FEATURE_JSON, REPLAY_REQUIRE_RUN_TAP, BEEP_DEV, BEEP_ENABLED
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --rounds)
      ROUNDS="$2"
      shift 2
      ;;
    --duration|--seconds|--secs)
      SOF_SECS="$2"
      shift 2
      ;;
    --out-dir)
      OUT_DIR="$2"
      shift 2
      ;;
    --variants)
      VARIANTS="$2"
      shift 2
      ;;
    --no-capture)
      DO_CAPTURE=0
      shift
      ;;
    --no-magnolia)
      DO_MAGNOLIA=0
      shift
      ;;
    --no-replay)
      DO_REPLAY=0
      shift
      ;;
    --no-analyze)
      DO_ANALYZE=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "ERROR: unknown arg: $1" >&2
      usage
      exit 1
      ;;
  esac
done

# =========================
# Summary helpers
# =========================
extract_max_field() {
  local log="$1"
  local field="$2"
  local pattern="$3"
  rg -n "$pattern" "$log" 2>/dev/null | sed -n "s/.*${field}=\\([0-9]\\+\\).*/\\1/p" | awk 'max<$1{max=$1} END{if (max=="") max=0; print max}'
}

extract_last_field() {
  local log="$1"
  local field="$2"
  local pattern="$3"
  rg -n "$pattern" "$log" 2>/dev/null | tail -1 | sed -n "s/.*${field}=\\([0-9]\\+\\).*/\\1/p"
}

pick_latest_feature_json() {
  local tap_root="$1"
  find "$tap_root" -name 'tap_FEATURES.json' -printf '%T@ %p\n' 2>/dev/null | sort -n | tail -1 | cut -d' ' -f2-
}

variant_env() {
  local variant="$1"
  local v_punct="$PARAKEET_DISABLE_PUNCT_SUPPRESSION"
  local v_cache="$PARAKEET_DISABLE_CACHE"
  case "$variant" in
    base)
      ;;
    nopunct)
      v_punct=1
      ;;
    nocache)
      v_cache=1
      ;;
    nocache_nopunct)
      v_cache=1
      v_punct=1
      ;;
    *)
      echo "ERROR: unknown variant: $variant" >&2
      exit 1
      ;;
  esac
  echo "$v_cache" "$v_punct"
}

# =========================
# Preflight
# =========================
require_cmd rg
require_cmd arecord
require_cmd timeout
mkdir -p "$OUT_DIR"

# =========================
# Main loop
# =========================
SUITE_ID="suite_$(ts)_pid$$"
SUITE_DIR="$OUT_DIR/$SUITE_ID"
mkdir -p "$SUITE_DIR"
SUMMARY_TSV="$SUITE_DIR/summary.tsv"
printf "variant\tround\tsof_wav\tmagn_enc_nan_max\tmagn_joint_nan_max\tmagn_text_len_max\tmagn_cache_len_last\treplay_tokens_max\treplay_text_len_max\treplay_blank_pref_last\treplay_nonblank_pref_last\tflags\n" > "$SUMMARY_TSV"

for variant in $VARIANTS; do
  for i in $(seq 1 "$ROUNDS"); do
    RUN_DIR="$SUITE_DIR/$variant/round_$i"
    mkdir -p "$RUN_DIR"
    say "Variant $variant round $i/$ROUNDS -> $RUN_DIR"

    export AUDIO_TAP_ENABLE=1
    export AUDIO_TAP_DIR="$RUN_DIR/taps"

    # -------------
    # 1) SOF capture
    # -------------
    if [[ "$DO_CAPTURE" == "1" ]]; then
      say "Capturing SOF audio: dev=$SOF_DEV rate=$SOF_RATE ch=$SOF_CH fmt=$SOF_FMT secs=$SOF_SECS"
      beep_times 1
      # Use hw for exact format; if this fails, switch to plughw:CARD=sofhdadsp,DEV=7
      arecord -D "$SOF_DEV" -f "$SOF_FMT" -r "$SOF_RATE" -c "$SOF_CH" -d "$SOF_SECS" \
        "$RUN_DIR/sof_capture_${SOF_RATE}hz_${SOF_CH}ch.wav" \
        2> "$RUN_DIR/sof_capture.stderr" || true
      beep_times 2
      tail -50 "$RUN_DIR/sof_capture.stderr" || true
    fi

    variant_vals="$(variant_env "$variant")"
    variant_cache="$(echo "$variant_vals" | awk '{print $1}')"
    variant_punct="$(echo "$variant_vals" | awk '{print $2}')"

    # ----------------
    # 2) Magnolia run
    # ----------------
    if [[ "$DO_MAGNOLIA" == "1" ]]; then
      say "Running Magnolia (timeout ${MAGNOLIA_TIMEOUT}s) with taps enabled"
      (
        cd "$MAGNOLIA_DIR"
        timeout "${MAGNOLIA_TIMEOUT}s" env \
          LD_LIBRARY_PATH="$TRT_ENGINE_LIB" \
          PARAKEET_DEBUG_SYNC="$PARAKEET_DEBUG_SYNC" \
          PARAKEET_DEBUG_SYNC_MEMCPY="$PARAKEET_DEBUG_SYNC_MEMCPY" \
          PARAKEET_DEBUG_BLANK_SCAN="$PARAKEET_DEBUG_BLANK_SCAN" \
          PARAKEET_DEBUG_EMIT_TOKENS="$PARAKEET_DEBUG_EMIT_TOKENS" \
          PARAKEET_DISABLE_PUNCT_SUPPRESSION="$variant_punct" \
          PARAKEET_DISABLE_CACHE="$variant_cache" \
          PARAKEET_Y0_OVERRIDE="$PARAKEET_Y0_OVERRIDE" \
          RUST_LOG="$RUST_LOG_MAGNOLIA" \
          cargo run -p daemon \
          >"$RUN_DIR/magnolia.log" 2>&1 || true
      )
      summarize_log "$RUN_DIR/magnolia.log"
      rg -n "NAN_GUARD ALERT" "$RUN_DIR/magnolia.log" | head -1 > "$RUN_DIR/first_nan.txt" || true
    fi

    # -------------------
    # 3) Feature replay
    # -------------------
    if [[ "$DO_REPLAY" == "1" ]]; then
      run_feature_json="$(pick_latest_feature_json "$RUN_DIR/taps" || true)"
      if [[ -z "$run_feature_json" ]]; then
        if [[ "$REPLAY_REQUIRE_RUN_TAP" == "1" ]]; then
          say "No tap_FEATURES.json found for this round; skipping replay"
          echo "no tap_FEATURES.json found" > "$RUN_DIR/feature_replay.log"
        else
          run_feature_json="$FEATURE_JSON"
        fi
      fi
      if [[ -n "$run_feature_json" ]]; then
        say "Running feature replay from $run_feature_json"
        (
          cd /home/emmy/git/trt-asr-engine/rust
          cargo build -p cli >/dev/null 2>&1 || true
          env \
            LD_LIBRARY_PATH="$TRT_ENGINE_LIB" \
            PARAKEET_DEBUG_BLANK_SCAN="$PARAKEET_DEBUG_BLANK_SCAN" \
            PARAKEET_DEBUG_EMIT_TOKENS="$PARAKEET_DEBUG_EMIT_TOKENS" \
            PARAKEET_DISABLE_PUNCT_SUPPRESSION="$variant_punct" \
            PARAKEET_DISABLE_CACHE="$variant_cache" \
            PARAKEET_Y0_OVERRIDE="$PARAKEET_Y0_OVERRIDE" \
            "$TRT_CLI_BIN" \
              "$run_feature_json" \
              --features-input \
              --model-dir "$MODEL_DIR" \
              -v \
              >"$RUN_DIR/feature_replay.log" 2>&1 || true
        )
        summarize_log "$RUN_DIR/feature_replay.log"
      fi
    fi

    # --------------------
    # 4) Optional analysis
    # --------------------
    if [[ "$DO_ANALYZE" == "1" ]]; then
      if command -v python >/dev/null 2>&1 && [[ -f /home/emmy/git/trt-asr-engine/tools/analyze_tap.py ]]; then
        say "Analyzing taps with analyze_tap.py"
        python /home/emmy/git/trt-asr-engine/tools/analyze_tap.py "$RUN_DIR/taps"/*/*.raw --compare \
          >"$RUN_DIR/analyze_taps.log" 2>&1 || true
        tail -80 "$RUN_DIR/analyze_taps.log" || true
      else
        say "Skipping analysis (python or analyze_tap.py not found)"
      fi
    fi

    # --------------------
    # 5) Summary row
    # --------------------
    magn_log="$RUN_DIR/magnolia.log"
    replay_log="$RUN_DIR/feature_replay.log"
    enc_nan_max=""
    joint_nan_max=""
    magn_text_len_max=""
    magn_cache_len=""
    replay_tokens_max=""
    replay_text_len_max=""
    blank_pref_last=""
    nonblank_pref_last=""
    if [[ -f "$magn_log" ]]; then
      enc_nan_max="$(extract_max_field "$magn_log" "nan_ct" "enc_out_stats")"
      joint_nan_max="$(extract_max_field "$magn_log" "nan_ct" "joint_out_stats")"
      magn_text_len_max="$(extract_max_field "$magn_log" "text_len" "emit_summary")"
      magn_cache_len="$(extract_last_field "$magn_log" "value" "cache_len_in value=")"
    fi
    if [[ -f "$replay_log" ]]; then
      replay_tokens_max="$(extract_max_field "$replay_log" "tokens" "emit_summary")"
      replay_text_len_max="$(extract_max_field "$replay_log" "text_len" "emit_summary")"
      blank_pref_last="$(extract_last_field "$replay_log" "blank_pref" "blank_scan")"
      nonblank_pref_last="$(extract_last_field "$replay_log" "nonblank_pref" "blank_scan")"
    fi
    flags=()
    if [[ -n "$enc_nan_max" && "$enc_nan_max" -gt 0 ]]; then flags+=("ENC_NAN"); fi
    if [[ -n "$joint_nan_max" && "$joint_nan_max" -gt 0 ]]; then flags+=("JOINT_NAN"); fi
    if [[ -n "$magn_text_len_max" && "$magn_text_len_max" -gt 0 ]]; then flags+=("MAGN_TEXT"); fi
    if [[ -n "$replay_tokens_max" && "$replay_tokens_max" -gt 0 ]]; then flags+=("REPLAY_TOKENS"); fi
    if [[ -n "$replay_text_len_max" && "$replay_text_len_max" -gt 0 ]]; then flags+=("REPLAY_TEXT"); fi
    if [[ ${#flags[@]} -eq 0 ]]; then
      flags=("OK")
    fi
    sof_wav="$RUN_DIR/sof_capture_${SOF_RATE}hz_${SOF_CH}ch.wav"
    printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n" \
      "$variant" "$i" "$sof_wav" \
      "${enc_nan_max:-}" "${joint_nan_max:-}" "${magn_text_len_max:-}" "${magn_cache_len:-}" \
      "${replay_tokens_max:-}" "${replay_text_len_max:-}" "${blank_pref_last:-}" "${nonblank_pref_last:-}" \
      "$(IFS=,; echo "${flags[*]}")" >> "$SUMMARY_TSV"

    say "Round $i complete. Artifacts in: $RUN_DIR"
  done
done

say "All rounds complete. Suite output: $SUITE_DIR"
if command -v column >/dev/null 2>&1; then
  echo
  echo "Summary:"
  column -t -s $'\t' "$SUMMARY_TSV"
fi
