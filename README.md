# Magnolia

Magnolia is a foundational connectivity layer for modular signal-processing systems. It provides a high-performance microkernel and a patch-bay style runtime that decouples data sources, processors, and sinks.

## Key Features

- **Microkernel Architecture**: Everything is a module.
- **Dynamic Plugins**: Load modules (`.so`/`.dll`) at runtime with hot-reloading support.
- **Low-Latency Audio**: Lock-free SPSC ring buffers for real-time DSP.
- **Secure**: Sandboxing (Linux) and Ed25519 signature verification for plugins.
- **Visualization Host**: Nannou-based visual runtime with configurable overlays.

## Structure

- **Core**
    - `magnolia_core`: The kernel, defining `Signal`, `ModuleRuntime`, and `PluginManager`.
    - `magnolia-plugin-abi`: Stable C interface for plugins.

- **Crates**
    - `audio_input`: Real-time audio source.
    - `audio_output`: Real-time audio sink.
    - `audio_dsp`: Audio processing utilities.
    - `text_tools`: Text analysis sinks.

- **Apps**
    - `daemon`: The Nannou-based visual engine and host.

## Getting Started

1. **Run the Daemon**:
   ```bash
   cargo run -p daemon
   ```

2. **Add a Plugin**:
   Drop a compiled plugin (`.so` or `.dll`) into the `./plugins` directory. The daemon will detect and load it automatically.

   See `examples/hello_plugin` to create your own.

## ASR Test Harness (Parakeet TRT)

The ASR smoke harness lives at `apps/asr_test` and depends on the C++ TRT runtime
now located in `/home/emmy/git/trt-asr-engine`.

1. **Build the C++ runtime**:
   ```bash
   cd /home/emmy/git/trt-asr-engine
   cmake -S cpp -B cpp/build
   cmake --build cpp/build -j
   ```
   If you keep the runtime elsewhere, set `PARAKEET_CPP_BUILD_DIR` accordingly.

2. **Run a smoke sweep (LibriSpeech dev-clean)**:
   ```bash
   cd /home/emmy/git/magnolia
   export LD_LIBRARY_PATH=/home/emmy/git/trt-asr-engine/cpp/build:${LD_LIBRARY_PATH}
   cargo run --bin asr_test -- \
     --dataset /home/emmy/git/magnolia/tools/LibriSpeech/dev-clean \
     --engine parakeet \
     --model-dir /home/emmy/git/trt-asr-engine/models/parakeet-tdt-0.6b-v3 \
     --mode smoke \
     --smoke-n 20 \
     --smoke-seed 123 \
     --blank-penalty 0.5 \
     --eos-pad-ms 0 \
     --utterance-timeout-ms 20000 \
     --inflight-chunks 1
   ```

   Debug knobs (optional):
   - `PARAKEET_SLOW_CHUNK_MS` (default 250): log per-chunk decode calls slower than this threshold.
   - `PARAKEET_ABORT_SLOW_CHUNK_MS` (default 5000): emit `slow_chunk_abort` error after a slow chunk returns.
   - `PARAKEET_WORKER_JOIN_TIMEOUT_MS` (default 0): cap worker join during restarts (ms).

## Keyboard Controls

Magnolia is keyboard-first with smart tile navigation:

| Key | Normal Mode | Layout Mode |
|-----|-------------|-------------|
| **Arrows** | Navigate between tiles (smart adjacency) | Navigate grid cursor |
| **E** | Open settings (if tile selected) / Enter layout | Enter resize mode |
| **P** | Enter patch mode | — |
| **Space** | — | Toggle resize ↔ move mode |
| **Enter** | — | Confirm resize/move |
| **ESC** | Deselect tile / Exit mode | Cancel / Exit mode |


## Configuration

- **Layout**: `configs/layout.toml` controls the visual grid.
- **Security**: 
  - `~/.magnolia/trusted_keys.txt`: Add Ed25519 public keys to verify signed plugins.
