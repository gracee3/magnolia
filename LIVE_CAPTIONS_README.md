# Magnolia Live Captions (Parakeet TRT Streaming Encoder)

This documents the v1 wiring for low-latency live captions using the validated
TensorRT streaming encoder and the Magnolia tile pipeline.

## Quick Start

1) Verify the TRT artifacts:
- `out/trt_engines/encoder_streaming_fp32.plan`
- `models/parakeet-tdt-0.6b-v3/{encoder.engine,predictor.engine,joint.engine,vocab.txt}`

2) Confirm `configs/layout.toml` has the STT config:
```
[parakeet_stt]
model_dir = "/home/emmy/git/trt-asr-engine/models/parakeet-tdt-0.6b-v3"
streaming_encoder_path = "/home/emmy/git/trt-asr-engine/out/trt_engines/encoder_streaming_fp32.plan"
use_fp16 = false
chunk_frames = 592
advance_frames = 8
```

3) Run Magnolia:
```
cargo run -p daemon
```

Expected behavior:
- Live captions update continuously in the `transcription` tile.
- The `parakeet_stt` tile shows STT status and latency.
- Logs emit `[transcription] latency_ms ...` summaries every N updates.

## Runtime Contract (Hard Fail)

The streaming encoder is **chunk-isolated** and validated under:
- `encoded_lengths == 1`
- `encoder_output.shape[-1] == 1`
- `cache_last_channel_len_out == 0`

These assertions are enforced on every encoder step in
`/home/emmy/git/trt-asr-engine/cpp/src/parakeet_trt.cpp`. Any violation throws,
emits an error event, and should be treated as a hard failure (config drift).

## Live Captions Behavior (v1)

Transcript state is split into:
- **Committed text**: stable prefix (never rewritten)
- **Partial text**: revisable suffix

Revision window defaults (overridable by env):
- `PARAKEET_REVISION_TOKENS=8`
- `PARAKEET_REVISION_STABLE_UPDATES=3`
- `PARAKEET_REVISION_MAX_AGE_MS=1500`
- `PARAKEET_UI_MIN_UPDATE_MS=40` (UI cadence throttle)

Endpoint/VAD emits a segment boundary and commits the current partial.

UI cadence tuning (hot):
- Transcription tile controls: `Up/Down` adjust cadence in 20ms steps, `R` resets to 40ms.
- Tile settings: `ui_min_update_ms` (persisted in `configs/layout.toml` when saved).

## Latency Instrumentation

Metrics emitted/logged:
- End-to-end latency (`audio timestamp -> UI update`) plus per-stage breakdown in `[transcription] latency_ms ...`
- Queue/backpressure + UI throttle counters in `[transcription] queues ...`
- Per-flush latency traces in `[transcription] latency_trace ...` (enable with `RUST_LOG=debug`)
- STT latency + decode latency from `stt_metrics`
- Slow chunk + queue delay warnings via `stt_slow_chunk`

Tuning env vars:
- `PARAKEET_LATENCY_REPORT_EVERY=20`
- `PARAKEET_LATENCY_REPORT_INTERVAL_MS=5000`
- `PARAKEET_LATENCY_MAX_SAMPLES=200`

## FP16 Toggle

Set `use_fp16 = true` in `configs/layout.toml` to enable FP16.
Expect faster inference with accuracy tradeoff (see TRT parity results).

## Demo Path (v1)

1) Start the daemon: `cargo run -p daemon`
2) Speak into the selected input device.
3) Watch the `transcription` tile update in real-time.
4) Use the `parakeet_stt` tile to stop/start/reset if needed (`S`, `T`, `R`).
