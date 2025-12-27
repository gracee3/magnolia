# TALISMAN // AGENT HAND-OFF

## 1. Project Overview
**Talisman** is a high-performance, event-driven daemon designed for real-time signal processing, astrological data retrieval, and generative ritual geometry. It uses a "Patch Bay" architecture to decouple data sources from visual sinks.

## 2. System Architecture (The Patch Bay)
The core logic resides in an asynchronous orchestrator using **Tokio** channels.

- **Signals (`talisman_core::Signal`)**: A unified enum that flows through the system.
  - `Text(String)`: Raw user input.
  - `Intent(String)`: Sanitized/Processed intent.
  - `Astrology(String)`: Real-time planetary state.
  - `Computed { source, content }`: Feedback from sinks back to the UI.
- **Sources**: Crates that emit signals (e.g., `Aphrodite` for stars, `Logos` for text).
- **Sinks**: Crates that consume signals (e.g., `Kamea` for sigils, `WordCount` for stats).

## 3. Crate Breakdown
- **`talisman_core`**: The backbone. Defines the `Signal` enum and `Source`/`Sink` traits.
- **`crates/aphrodite`**: Wraps the Swiss Ephemeris (via `swisseph`). Provides high-precision astrological "Salting".
- **`crates/logos`**: Handles the ingestion of intent. Currently supports stdin and async polling.
- **`crates/kamea`**: Generates grid-based geometry (Sigils) using SHA-256 hashes of intents as seeds for deterministic random paths.
- **`crates/text_tools`**: Real-time analysis sinks (Word Count, Devowelizer).
- **`apps/daemon`**: The central orchestrator and GUI.
  - **Engine**: Nannou (WGPU-based graphics).
  - **GUI**: Egui (Integrated for text editing).

## 4. Key Features for Developers
- **Layout Engine**: Driven by `configs/layout.toml`. Supports percentage, pixel, and fractional (`fr`) tracks in a grid system.
- **Immersive UI**:
  - **Transparency**: The Egui text editor is fully transparent, floating "inside" the ritual space.
  - **Maximization**: Double-click any tile to zoom it to full-screen. Uses Cubic Ease-In-Out lerping for the animation.
  - **Retinal Burn**: A global inversion state (`model.retinal_burn`) that flips the color palette (Cyan/Black).
- **Clipboard Management**: Tiles are selectable via mouse click. `Ctrl+C` copies a module's specific text representation, and `Ctrl+V` pastes into the editor.

## 5. Focus Areas for Future Agents
- **Visuals**: Implement WGPU shaders for the "Retinal Burn" effect (e.g., scanlines, chromatic aberration).
- **Stability**: Enhance the `Signal::Computed` feedback loop to handle high-bandwidth data (e.g., audio spectrums).
- **Layout**: Add a "Drag and Drop" mode to edit `layout.toml` visually within the daemon.
- **Connectivity**: Implement an OSC or WebSocket source/sink for cross-machine synchronization.
- **Modularity**: Generalize the `render_tile` function to allow modules to register custom drawing closures.

## 6. How to Run
```bash
# Production mode (Smart logs)
cargo run -p daemon

# Debug mode (Verbose logs)
RUST_LOG=debug cargo run -p daemon
```
