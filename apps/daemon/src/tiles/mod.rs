//! Tile System - Unified tile rendering and management
//!
//! This module re-exports the TileRenderer trait from talisman_core
//! and tile implementations from their respective crates.
//!
//! ## Architecture
//! - **Monitor Mode**: Normal tile view, read-only feedback display
//! - **Control Mode**: Maximized tile view, settings UI with live preview
//! - **Error Handling**: Tiles can report errors displayed in monitor view



// Local tile implementations (remaining - clock is still local)
pub mod clock;
pub mod gpu_renderer;

// Re-export main types from talisman_core
pub use talisman_core::{
    TileRenderer, 
    TileRegistry, 
    RenderContext, 
 
    BindableAction,
    render_error_overlay,
};

// Re-export GpuRenderer (daemon-specific)
pub use gpu_renderer::GpuRenderer;

// Re-export tiles from crates
pub use kamea::KameaTile;
pub use aphrodite::AstroTile;
pub use logos::TextInputTile;
pub use audio_input::AudioVisTile;

/// Create the default tile registry with demo tiles
pub fn create_default_registry() -> TileRegistry {
    let mut registry = TileRegistry::new();
    
    // Register demo tiles
    registry.register(clock::ClockTile::new());
    registry.register(AstroTile::new());
    registry.register(TextInputTile::new());
    registry.register(KameaTile::new());
    registry.register(AudioVisTile::new("audio_vis"));
    
    registry
}
