# MAGNOLIA // AGENT HAND-OFF

## 1. Project Overview
**Magnolia** is a high-performance, modular microkernel for real-time signal processing, astrological data retrieval, and generative ritual geometry. It uses a "Patch Bay" architecture with a dynamic plugin system to decouple data sources from visual sinks.

## 2. System Architecture (The Patch Bay)
The core logic resides in an asynchronous orchestrator using **Tokio** channels and dynamically loaded modules.

- **Signals (`magnolia_core::Signal`)**: A unified enum that flows through the system.
  - `Text(String)`: Raw user input.
  - `Intent(String)`: Sanitized/Processed intent.
  - `Astrology(String)`: Real-time planetary state.
  - `AudioStream(RingBufferReceiver)`: Lock-free SPSC channel for low-latency audio.
  - `SharedAudio(Arc<AudioData>)`: Zero-copy large audio buffers.
- **Modules**: `Source`, `Sink`, or `Processor` types that can be loaded statically or dynamically.

## 3. Crate Breakdown
- **`magnolia_core`**: The backbone. Defines core traits, `ModuleHost`, `PluginManager`, and `Signal` types.
- **`crates/magnolia-plugin-abi`**: Stable C ABI definition for cross-language/version dynamic plugins.
- **`crates/aphrodite`**: Wraps the Swiss Ephemeris. Provides high-precision astrological "Salting".
- **`crates/logos`**: Handles the ingestion of intent.
- **`crates/kamea`**: Generates grid-based geometry (Sigils).
- **`crates/audio_input`**: Real-time audio ingestion using CPAL and SPSC Ring Buffers.
- **`apps/daemon`**: The central orchestrator and GUI (Nannou + Egui).

## 4. Tile System Architecture (New)

The tile system separates **monitor mode** (read-only display) from **control mode** (settings UI).

### TileRenderer Trait (`apps/daemon/src/tiles/mod.rs`)
```rust
pub trait TileRenderer: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    
    // Rendering modes
    fn render_monitor(&self, draw: &Draw, rect: Rect, ctx: &RenderContext);  // Grid view
    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool;  // Maximized
    
    // Settings
    fn settings_schema(&self) -> Option<serde_json::Value>;
    fn apply_settings(&mut self, settings: &serde_json::Value);
    fn get_settings(&self) -> serde_json::Value;
    
    // Keybind actions
    fn bindable_actions(&self) -> Vec<BindableAction>;
    fn execute_action(&mut self, action: &str) -> bool;
    
    // Error handling
    fn get_error(&self) -> Option<TileError>;
    fn clear_error(&mut self);
    
    // Lifecycle
    fn update(&mut self);
    fn prefers_gpu(&self) -> bool;
}
```

### Error Reporting
Tiles can report errors via `TileError`:
```rust
pub struct TileError {
    pub message: String,
    pub details: Option<String>,
    pub severity: ErrorSeverity,  // Info, Warning, Error
}
```

### Audio Visualization
`AudioVisTile` uses SPSC ring buffer for minimal latency:
- `connect_audio_stream(RingBufferReceiver<AudioFrame>)` - Wire audio source
- Supports oscilloscope, spectrum bars, VU meter, Lissajous visualizations
- Keybind actions: mute, freeze, next_vis

### Per-Tile Settings (`configs/layout.toml`)
```toml
[[tiles]]
id = "audio_vis_1"
module = "audio_vis"

[tiles.settings]
config = { vis_type = "Oscilloscope", color_scheme = "CyanReactive" }
keybinds = { mute = "m", freeze = "f", next_vis = "n" }
```

## 5. Key Features for Developers
- **Dynamic Plugin System**:
  - Load `.so`/`.dll` plugins at runtime from `./plugins` or `~/.magnolia/plugins`.
  - **Hot-Reloading**: Plugins auto-reload on file change during development.
  - **Sandboxing**: Linux plugins restricted via `seccomp-bpf`.
  - **Signing**: Optional Ed25519 signature verification.
- **Low-Latency Audio**:
  - Dedicated `SPSCRingBuffer` for lock-free audio streaming (~5-10ns per frame).
  - `AudioFrame` structures for optimized memory layout.
  - `AudioInputSourceRT::new()` returns `(source, RingBufferReceiver)` tuple.
- **Layout Engine**: `configs/layout.toml` drives the grid system with percentage/pixel/fr tracks.
- **Tile Settings**: Per-instance configuration with keybinds, persisted to TOML.
- **GPU Rendering**: `GpuRenderer` for hardware-accelerated visualizations.
- **Keyboard-First Navigation** (`apps/daemon/src/input.rs`):
  - **Smart Tile Navigation**: Arrow keys use adjacency detection with overlap calculation
  - **InputMode State Machine**: `Normal`, `Layout`, `Patch` modes with ESC cascade
  - **Layout Editing**: Resize (arrows) and Move (Space toggle) modes with Enter to confirm
  - Cursor position persists between modes
  - Single key press jumps between tiles (no double-press issue)


## 6. Focus Areas for Future Agents
- **Security Audit**: Verify capability-based security model and sandbox rules.
- **Audio Processing**: Implement DSP processor modules using the Ring Buffer system.
- **FFT Spectrum**: Add real FFT to AudioVisTile for proper spectrum analysis.
- **Marketplace**: Build a registry for community plugins.
- **Tile Instances**: Support multiple instances of the same tile type with different IDs.

## 7. How to Run
```bash
# Production mode (Smart logs)
cargo run -p daemon

# Debug mode (Verbose logs + Hot Reload enabled)
RUST_LOG=debug cargo run -p daemon
```

## 8. Plugin Development
See `examples/hello_plugin` for a minimal C ABI plugin implementation.
To build a plugin:
```bash
cargo build --release -p my_plugin
cp target/release/libmy_plugin.so plugins/
```
