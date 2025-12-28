//! Tile System - Unified tile rendering and management
//!
//! This module re-exports the TileRenderer trait from talisman_core
//! and provides local tile implementations.
//!
//! ## Architecture
//! - **Monitor Mode**: Normal tile view, read-only feedback display
//! - **Control Mode**: Maximized tile view, settings UI with live preview
//! - **Error Handling**: Tiles can report errors displayed in monitor view

use nannou::prelude::*;

// Local tile implementations
pub mod clock;
pub mod astrology;
pub mod text_input;
pub mod gpu_renderer;
pub mod audio_vis;

// Re-export main types from talisman_core
pub use talisman_core::{
    TileRenderer, 
    TileRegistry, 
    RenderContext, 
    TileError, 
    ErrorSeverity, 
    BindableAction,
    render_error_overlay,
};

// Re-export GpuRenderer (daemon-specific)
pub use gpu_renderer::GpuRenderer;

// Re-export tiles from crates
pub use kamea::KameaTile;

/// Create the default tile registry with demo tiles
pub fn create_default_registry() -> TileRegistry {
    let mut registry = TileRegistry::new();
    
    // Register demo tiles
    registry.register(clock::ClockTile::new());
    registry.register(astrology::AstroTile::new());
    registry.register(text_input::TextInputTile::new());
    registry.register(KameaTile::new());
    registry.register(audio_vis::AudioVisTile::new("audio_vis"));
    
    registry
}
