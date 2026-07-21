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
