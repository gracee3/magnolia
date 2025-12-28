//! Tile System - Unified tile rendering and management
//!
//! This module provides the TileRenderer trait and a central registry
//! for all available tiles in the application.
//!
//! ## Architecture
//! - **Monitor Mode**: Normal tile view, read-only feedback display
//! - **Control Mode**: Maximized tile view, settings UI with live preview

use nannou::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub mod clock;
pub mod astrology;
pub mod text_input;
pub mod kamea;
pub mod gpu_renderer;
pub mod audio_vis;

pub use gpu_renderer::GpuRenderer;

/// Describes an action that can be bound to a key
#[derive(Debug, Clone)]
pub struct BindableAction {
    /// Unique action identifier (e.g., "mute")
    pub id: String,
    /// Human-readable label (e.g., "Mute Audio")
    pub label: String,
    /// Whether this is a toggle (true) or momentary (false) action
    pub is_toggle: bool,
}

impl BindableAction {
    pub fn new(id: &str, label: &str, is_toggle: bool) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            is_toggle,
        }
    }
}

/// Context passed to tiles during rendering
pub struct RenderContext<'a> {
    pub time: std::time::Instant,
    pub frame_count: u64,
    pub is_selected: bool,
    pub is_maximized: bool,
    pub egui_ctx: Option<&'a nannou_egui::egui::Context>,
    /// Per-tile settings from config (read-only access during render)
    pub tile_settings: Option<&'a serde_json::Value>,
    /// GPU renderer for hardware-accelerated visualization
    pub gpu: Option<&'a GpuRenderer>,
}

impl<'a> RenderContext<'a> {
    pub fn new() -> Self {
        Self {
            time: std::time::Instant::now(),
            frame_count: 0,
            is_selected: false,
            is_maximized: false,
            egui_ctx: None,
            tile_settings: None,
            gpu: None,
        }
    }
}

impl<'a> Default for RenderContext<'a> {
    fn default() -> Self {
        Self::new()
    }
}

/// Core trait for all renderable tiles
/// 
/// Tiles have two rendering modes:
/// - `render_monitor`: Read-only display for normal tile view
/// - `render_controls`: Settings UI for maximized tile view
pub trait TileRenderer: Send + Sync {
    // === IDENTITY ===
    
    /// Unique identifier for this tile type
    fn id(&self) -> &str;
    
    /// Human-readable name
    fn name(&self) -> &str;
    
    // === RENDERING ===
    
    /// Render monitor view (small tile, read-only feedback)
    /// 
    /// This is the default view shown in the grid. Should display
    /// current state/feedback without any interactive controls.
    fn render_monitor(&self, draw: &Draw, rect: Rect, ctx: &RenderContext);
    
    /// Render control view (maximized, includes settings UI)
    /// 
    /// This is shown when the tile is maximized (double-click/Enter).
    /// Should include settings controls and a live preview of the tile.
    /// 
    /// Returns true if egui was used (for input routing).
    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        // Default: just render monitor view
        self.render_monitor(draw, rect, ctx);
        false
    }
    
    /// Whether this tile prefers GPU-accelerated rendering
    fn prefers_gpu(&self) -> bool { false }
    
    // === LIFECYCLE ===
    
    /// Update tile state (called each frame before render)
    fn update(&mut self);
    
    // === SETTINGS ===
    
    /// JSON Schema describing available settings
    /// 
    /// Used to generate settings UI in control mode.
    /// Returns None if this tile has no configurable settings.
    fn settings_schema(&self) -> Option<serde_json::Value> { None }
    
    /// Apply settings from persisted config
    /// 
    /// Called when loading layout or when settings are changed in UI.
    fn apply_settings(&mut self, _settings: &serde_json::Value) {}
    
    /// Get current settings as JSON for persistence
    fn get_settings(&self) -> serde_json::Value { 
        serde_json::Value::Null 
    }
    
    // === KEYBINDS ===
    
    /// List of action names that can be bound to keys
    /// 
    /// Returns descriptions of available actions like mute, freeze, etc.
    fn bindable_actions(&self) -> Vec<BindableAction> { vec![] }
    
    /// Execute a bound action, returns true if handled
    fn execute_action(&mut self, _action: &str) -> bool { false }
    
    // === LEGACY (deprecated, use render_monitor) ===
    
    /// Render the tile content (deprecated, use render_monitor)
    #[deprecated(note = "Use render_monitor instead")]
    fn render(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) {
        self.render_monitor(draw, rect, ctx);
    }
    
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
    
    /// Render a tile in monitor mode by module name
    pub fn render_monitor(&self, module: &str, draw: &Draw, rect: Rect, ctx: &RenderContext) {
        if let Some(tile) = self.tiles.get(module) {
            if let Ok(t) = tile.read() {
                t.render_monitor(draw, rect, ctx);
            }
        }
    }
    
    /// Render a tile in control mode by module name
    /// Returns true if egui was used
    pub fn render_controls(&self, module: &str, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        if let Some(tile) = self.tiles.get(module) {
            if let Ok(t) = tile.read() {
                return t.render_controls(draw, rect, ctx);
            }
        }
        false
    }
    
    /// Render a tile by module name (legacy, uses monitor mode)
    #[deprecated(note = "Use render_monitor instead")]
    pub fn render(&self, module: &str, draw: &Draw, rect: Rect, ctx: &RenderContext) {
        self.render_monitor(module, draw, rect, ctx);
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
    
    /// Apply settings to a tile
    pub fn apply_settings(&self, module: &str, settings: &serde_json::Value) {
        if let Some(tile) = self.tiles.get(module) {
            if let Ok(mut t) = tile.write() {
                t.apply_settings(settings);
            }
        }
    }
    
    /// Get settings from a tile
    pub fn get_settings(&self, module: &str) -> serde_json::Value {
        if let Some(tile) = self.tiles.get(module) {
            if let Ok(t) = tile.read() {
                return t.get_settings();
            }
        }
        serde_json::Value::Null
    }
    
    /// Execute an action on a tile
    pub fn execute_action(&self, module: &str, action: &str) -> bool {
        if let Some(tile) = self.tiles.get(module) {
            if let Ok(mut t) = tile.write() {
                return t.execute_action(action);
            }
        }
        false
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
    registry.register(kamea::KameaTile::new());
    registry.register(audio_vis::AudioVisTile::new("audio_vis"));
    
    registry
}
