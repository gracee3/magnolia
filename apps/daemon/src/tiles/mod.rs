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
pub mod schema_tile;
pub use schema_tile::SchemaTile;
pub mod clock;
pub mod compositor;

// Re-export main types from talisman_core
pub use talisman_core::{
    TileRenderer, 
    TileRegistry, 
    RenderContext, 
 
    BindableAction,
    render_error_overlay,
};

// Re-export Compositor (daemon-specific)
pub use compositor::Compositor;

/// Create the default tile registry with local tiles only
/// External tiles must be loaded via PluginManager
pub fn create_default_registry() -> TileRegistry {
    let mut registry = TileRegistry::new();
    
    // Register local system tiles
    registry.register(clock::ClockTile::new());
    
    registry
}
