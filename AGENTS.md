# TALISMAN // AGENT HAND-OFF

## 1. Project Overview
**Talisman** is a high-performance, modular microkernel for real-time signal processing, astrological data retrieval, and generative ritual geometry. It uses a "Patch Bay" architecture with a dynamic plugin system to decouple data sources from visual sinks.

## 2. System Architecture (The Patch Bay)
The core logic resides in an asynchronous orchestrator using **Tokio** channels and dynamically loaded modules.

- **Signals (`talisman_core::Signal`)**: A unified enum that flows through the system.
  - `Text(String)`: Raw user input.
  - `Intent(String)`: Sanitized/Processed intent.
  - `Astrology(String)`: Real-time planetary state.
  - `AudioStream(RingBufferReceiver)`: Lock-free SPSC channel for low-latency audio.
  - `SharedAudio(Arc<AudioData>)`: Zero-copy large audio buffers.
- **Modules**: `Source`, `Sink`, or `Processor` types that can be loaded statically or dynamically.

## 3. Crate Breakdown
- **`talisman_core`**: The backbone. Defines core traits, `ModuleHost`, `PluginManager`, and `Signal` types.
- **`crates/talisman-plugin-abi`**: Stable C ABI definition for cross-language/version dynamic plugins.
- **`crates/aphrodite`**: Wraps the Swiss Ephemeris. Provides high-precision astrological "Salting".
- **`crates/logos`**: Handles the ingestion of intent.
- **`crates/kamea`**: Generates grid-based geometry (Sigils).
- **`crates/audio_input`**: Real-time audio ingestion using CPAL and Ring Buffers.
- **`apps/daemon`**: The central orchestrator and GUI (Nannou + Egui).

## 4. Key Features for Developers
- **Dynamic Plugin System**:
  - Load `.so`/`.dll` plugins at runtime from `./plugins` or `~/.talisman/plugins`.
  - **Hot-Reloading**: Plugins auto-reload on file change during development.
  - **Sandboxing**: Linux plugins restricted via `seccomp-bpf`.
  - **Signing**: Optional Ed25519 signature verification.
- **Low-Latency Audio**:
  - Dedicated `SPSCRingBuffer` for lock-free audio streaming.
  - `AudioFrame` structures for optimized memory layout.
- **Layout Engine**: `configs/layout.toml` drives the grid system with percentage/pixel/fr tracks.
- **Immersive UI**: Transparent floating editor, cubic easing animations, and "Retinal Burn" color modes.

## 5. Focus Areas for Future Agents
- **Security Audit**: Verify capability-based security model and sandbox rules.
- **Audio Processing**: Implement DSP processor modules using the new Ring Buffer system.
- **Layout Manager**: Implement drag-and-drop UI to edit `layout.toml` live (Phase 5).
- **Marketplace**: Build a registry for community plugins.

## 6. How to Run
```bash
# Production mode (Smart logs)
cargo run -p daemon

# Debug mode (Verbose logs + Hot Reload enabled)
RUST_LOG=debug cargo run -p daemon
```

## 7. Plugin Development
See `examples/hello_plugin` for a minimal C ABI plugin implementation.
To build a plugin:
```bash
cargo build --release -p my_plugin
cp target/release/libmy_plugin.so plugins/
```
