# Talisman

Talisman is a cybernetic practice suite for digital introspection and chaos magic, built on a high-performance modular microkernel.

It is a "Cybernetic Spirit" forged from three components:
1.  **Aphrodite** (Time) - Astrological Data
2.  **Logos** (Intent) - Input Processing
3.  **Kamea** (Geometry) - Sigil Generation

The **Daemon** is the vessel that binds them, orchestrating data flow through a dynamic Patch Bay.

## Key Features

- **Microkernel Architecture**: Everything is a module.
- **Dynamic Plugins**: Load modules (`.so`/`.dll`) at runtime with hot-reloading support.
- **Low-Latency Audio**: Lock-free SPSC ring buffers for real-time DSP.
- **Secure**: Sandboxing (Linux) and Ed25519 signature verification for plugins.
- **Visuals**: Nannou-based generative visuals with transparent overlays.

## Structure

- **Core**
    - `talisman_core`: The kernel, defining `Signal`, `ModuleRuntime`, and `PluginManager`.
    - `talisman-plugin-abi`: Stable C interface for plugins.

- **Crates**
    - `aphrodite`: Time & Astrology logic.
    - `logos`: Input, Intent, & Hashing logic.
    - `kamea`: Geometry & Grid logic.
    - `audio_input`: Real-time audio source.
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

## Configuration

- **Layout**: `configs/layout.toml` controls the visual grid.
- **Security**: 
  - `~/.talisman/trusted_keys.txt`: Add Ed25519 public keys to verify signed plugins.
