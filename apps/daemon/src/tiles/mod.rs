//! Tile System - Unified tile rendering and management
//!
//! This module provides the TileRenderer trait and a central registry
//! for all available tiles in the application.

use nannou::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub mod clock;
pub mod astrology;
pub mod text_input;

/// Context passed to tiles during rendering
pub struct RenderContext<'a> {
    pub time: std::time::Instant,
    pub frame_count: u64,
    pub is_selected: bool,
    pub is_maximized: bool,
    pub egui_ctx: Option<&'a nannou_egui::egui::Context>,
}

impl<'a> RenderContext<'a> {
    pub fn new() -> Self {
        Self {
            time: std::time::Instant::now(),
            frame_count: 0,
            is_selected: false,
            is_maximized: false,
            egui_ctx: None,
        }
    }
}

/// Core trait for all renderable tiles
pub trait TileRenderer: Send + Sync {
    /// Unique identifier for this tile type
    fn id(&self) -> &str;
    
    /// Human-readable name
    fn name(&self) -> &str;
    
    /// Render the tile content
    fn render(&self, draw: &Draw, rect: Rect, ctx: &RenderContext);
    
    /// Update tile state (called each frame before render)
    fn update(&mut self);
    
    /// Get current display text (for simple text-based tiles)
    fn get_display_text(&self) -> Option<String> { None }
}

/// Central registry for tile instances
pub struct TileRegistry {
    tiles: HashMap<String, Arc<RwLock<Box<dyn TileRenderer>>>>,
}

impl TileRegistry {
    pub fn new() -> Self {
        Self {
            tiles: HashMap::new(),
        }
    }
    
    /// Register a new tile instance
    pub fn register<T: TileRenderer + 'static>(&mut self, tile: T) {
        let id = tile.id().to_string();
        self.tiles.insert(id, Arc::new(RwLock::new(Box::new(tile))));
    }
    
    /// Get a tile by ID
    pub fn get(&self, id: &str) -> Option<Arc<RwLock<Box<dyn TileRenderer>>>> {
        self.tiles.get(id).cloned()
    }
    
    /// Update all tiles (call each frame)
    pub fn update_all(&self) {
        for tile in self.tiles.values() {
            if let Ok(mut t) = tile.write() {
                t.update();
            }
        }
    }
    
    /// Render a tile by module name
    pub fn render(&self, module: &str, draw: &Draw, rect: Rect, ctx: &RenderContext) {
        if let Some(tile) = self.tiles.get(module) {
            if let Ok(t) = tile.read() {
                t.render(draw, rect, ctx);
            }
        }
    }
    
    /// Get display text for a tile
    pub fn get_display_text(&self, module: &str) -> Option<String> {
        if let Some(tile) = self.tiles.get(module) {
            if let Ok(t) = tile.read() {
                return t.get_display_text();
            }
        }
        None
    }
}

impl Default for TileRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Create the default tile registry with demo tiles
pub fn create_default_registry() -> TileRegistry {
    let mut registry = TileRegistry::new();
    
    // Register demo tiles
    registry.register(clock::ClockTile::new());
    registry.register(astrology::AstroTile::new());
    registry.register(text_input::TextInputTile::new());
    
    registry
}
