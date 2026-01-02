#!/usr/bin/env bash
set -euo pipefail

OUT="${1:-/tmp/audio_debug_snapshot.txt}"

have() { command -v "$1" >/dev/null 2>&1; }

sec() {
  echo
  echo "========================================"
  echo "== $1"
  echo "========================================"
}

run() {
  # Run a command, but never fail the whole script if it fails.
  echo "\$ $*"
  ( "$@" ) || echo "[warn] command failed (exit $?): $*"
}

run_sh() {
  # Run shell snippet safely; never fail the whole script.
  echo "\$ bash -lc '$*'"
  ( bash -lc "$*" ) || echo "[warn] command failed (exit $?): bash -lc '$*'"
}

# Create output directory if needed
mkdir -p "$(dirname "$OUT")"

{
  sec "DATE"
  date

  sec "SYSTEM"
  run uname -a
  run_sh 'cat /etc/os-release 2>/dev/null || true'
  run_sh 'lscpu 2>/dev/null | sed -n "1,60p" || true'

  sec "AUDIO SERVICES (PipeWire/WirePlumber/Pulse)"
  run_sh 'systemctl --user status pipewire --no-pager 2>/dev/null || true'
  run_sh 'systemctl --user status wireplumber --no-pager 2>/dev/null || true'
  run_sh 'systemctl --user status pulseaudio --no-pager 2>/dev/null || true'

  if have pactl; then
    run pactl info
    run pactl list short sources
    run pactl list short sinks
  else
    echo "[info] pactl not found. If you want it: sudo apt-get install -y pulseaudio-utils"
    if have pw-cli; then
      run pw-cli info 0
    else
      echo "[warn] pw-cli not found; PipeWire CLI tools missing."
    fi
  fi

  sec "PIPEWIRE GRAPH (if available)"
  if have pw-cli; then
    run_sh 'pw-cli ls Node | sed -n "1,200p" || true'
    run_sh 'pw-cli ls Port | sed -n "1,200p" || true'
  else
    echo "[info] pw-cli not found. If you want it: sudo apt-get install -y pipewire-bin"
  fi

  if have pw-dump; then
    run_sh 'pw-dump > /tmp/pw-dump.json && echo "Wrote /tmp/pw-dump.json"'
    run_sh 'grep -iE "source|mic|echo|cancel|aec|beam|sof|thinkpad|intel" /tmp/pw-dump.json | head -n 200 || true'
  else
    echo "[info] pw-dump not found (usually comes with pipewire)."
  fi

  sec "PCI AUDIO DEVICES + KERNEL DRIVER BINDING"
  run_sh 'lspci -nnk | egrep -A3 -i "audio|multimedia" || true'

  sec "KERNEL MODULES (SOF/HDA/SND)"
  run_sh 'lsmod | egrep -i "snd|sof|hda|intel|sound" || true'

  sec "KERNEL LOGS (SOF/HDA) — dmesg if permitted, else journalctl"
  # Show the sysctl controlling dmesg access
  run_sh 'sysctl kernel.dmesg_restrict 2>/dev/null || true'

  if run_sh 'dmesg -T >/dev/null 2>&1'; then
    run_sh 'dmesg -T | egrep -i "sof|snd_sof|tplg|firmware|hda codec|dsp" | tail -n 250 || true'
  else
    echo "[info] dmesg not permitted for this user (kernel.dmesg_restrict may be 1)."
    if have journalctl; then
      run_sh 'journalctl -k --no-pager | egrep -i "sof|snd_sof|tplg|firmware|hda codec|dsp" | tail -n 250 || true'
    else
      echo "[warn] journalctl not available."
    fi
  fi

  sec "ALSA INVENTORY"
  run_sh 'cat /proc/asound/cards 2>/dev/null || true'
  run_sh 'cat /proc/asound/pcm 2>/dev/null || true'

  if have arecord; then
    run arecord -l
    run_sh 'arecord -L | sed -n "1,220p" || true'
  else
    echo "[warn] arecord not found. Install: sudo apt-get install -y alsa-utils"
  fi

  sec "CODEC INFO (if exposed)"
  run_sh 'ls /proc/asound/card*/codec* 2>/dev/null || true'
  run_sh 'cat /proc/asound/card*/codec* 2>/dev/null | sed -n "1,220p" || true'

  sec "ALSA MIXER CONTROLS (look for beamforming/AEC/AGC/NS)"
  if have amixer; then
    # Dump card0 controls; if multiple cards exist, agent can repeat with -c 1, -c 2.
    run amixer -c 0 scontrols
    run_sh 'amixer -c 0 contents > /tmp/amixer_card0.txt && echo "Wrote /tmp/amixer_card0.txt"'
    run_sh 'grep -iE "beam|array|dmic|ns|noise|aec|agc|boost|echo" /tmp/amixer_card0.txt | head -n 250 || true'
  else
    echo "[warn] amixer not found. Install: sudo apt-get install -y alsa-utils"
  fi

  sec "OPTIONAL: QUICK CAPTURE SMOKE TEST (only if arecord exists)"
  if have arecord; then
    run_sh 'arecord -d 3 -f S16_LE -r 48000 -c 2 /tmp/mic_test_48k_stereo.wav 2>/dev/null || true'
    run_sh 'arecord -d 3 -f S16_LE -r 16000 -c 1 /tmp/mic_test_16k_mono.wav 2>/dev/null || true'
    if have soxi; then
      run_sh 'soxi /tmp/mic_test_48k_stereo.wav 2>/dev/null || true'
      run_sh 'soxi /tmp/mic_test_16k_mono.wav 2>/dev/null || true'
    else
      echo "[info] soxi not found. Install: sudo apt-get install -y sox"
    fi
  fi

  sec "NOTES"
  echo "If dmesg was restricted, you can temporarily allow it with:"
  echo "  sudo sysctl -w kernel.dmesg_restrict=0"
  echo "Or just rely on journalctl -k output (preferred on Ubuntu with systemd)."
  echo
  echo "If pactl is missing and you want it:"
  echo "  sudo apt-get install -y pulseaudio-utils"
} > "$OUT"

echo "Wrote $OUT"
echo "Attach this file to your agent/debug thread."
