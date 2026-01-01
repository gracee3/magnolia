//! Tile Rendering System
//!
//! This module provides the `TileRenderer` trait and supporting types for
//! creating visual tiles in the Magnolia daemon.
//!
//! ## Feature Flag
//! This module is only available when the `tile-rendering` feature is enabled.
//! Module crates that wish to provide visual tiles should enable this feature:
//!
//! ```toml
//! [dependencies.magnolia_core]
//! path = "../../core"
//! features = ["tile-rendering"]
//! ```
//!
//! ## Architecture
//! - **Monitor Mode**: Normal tile view, read-only feedback display
//! - **Control Mode**: Maximized tile view, settings UI with live preview
//! - **Error Handling**: Tiles can report errors displayed in monitor view

use nannou::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use magnolia_ui::{draw_text, FontId, TextAlignment};

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
    pub power_profile: crate::PowerProfile,
    /// Per-tile settings from config (read-only access during render)
    pub tile_settings: Option<&'a serde_json::Value>,
}

impl<'a> RenderContext<'a> {
    pub fn new() -> Self {
        Self {
            time: std::time::Instant::now(),
            frame_count: 0,
            is_selected: false,
            is_maximized: false,
            power_profile: crate::PowerProfile::Normal,
            tile_settings: None,
        }
    }
}

impl<'a> Default for RenderContext<'a> {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents an error state that a tile can display
#[derive(Debug, Clone)]
pub struct TileError {
    /// Short error message (shown in monitor view)
    pub message: String,
    /// Detailed error info (shown in control view)
    pub details: Option<String>,
    /// Error severity level
    pub severity: ErrorSeverity,
    /// When the error occurred
    pub timestamp: std::time::Instant,
}

impl TileError {
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
            details: None,
            severity: ErrorSeverity::Error,
            timestamp: std::time::Instant::now(),
        }
    }

    pub fn with_details(mut self, details: &str) -> Self {
        self.details = Some(details.to_string());
        self
    }

    pub fn warning(message: &str) -> Self {
        Self {
            message: message.to_string(),
            details: None,
            severity: ErrorSeverity::Warning,
            timestamp: std::time::Instant::now(),
        }
    }

    pub fn info(message: &str) -> Self {
        Self {
            message: message.to_string(),
            details: None,
            severity: ErrorSeverity::Info,
            timestamp: std::time::Instant::now(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    Info,    // Blue - informational
    Warning, // Yellow - something might be wrong
    Error,   // Red - something is wrong
}

/// Core trait for all renderable tiles
///
/// Tiles have two rendering modes:
/// - `render_monitor`: Read-only display for normal tile view
/// - `render_controls`: Settings UI for maximized tile view
pub trait TileRenderer: Send + Sync {
    // === IDENTITY ===

    /// Unique identifier for this tile type (should match module ID)
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
    /// Returns true if input was handled.
    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        // Default: just render monitor view
        self.render_monitor(draw, rect, ctx);
        false
    }

    // === INPUT (optional) ===
    //
    // Tiles are rendered by Nannou, but keyboard routing is handled by the daemon.
    // When a tile is maximized, the daemon may forward key presses here so the tile
    // can implement custom controls (toggles, steppers, lists, etc).
    //
    // Returns true if the tile consumed the key.
    fn handle_key(&mut self, _key: nannou::prelude::Key, _ctrl: bool, _shift: bool) -> bool {
        false
    }

    /// Whether this tile prefers GPU-accelerated rendering
    fn prefers_gpu(&self) -> bool {
        false
    }

    // === ERROR HANDLING ===

    /// Get current error state, if any
    fn get_error(&self) -> Option<TileError> {
        None
    }

    /// Clear the current error
    fn clear_error(&mut self) {}

    // === LIFECYCLE ===

    /// Update tile state (called each frame before render)
    fn update(&mut self);

    // === SETTINGS ===

    /// JSON Schema describing available settings
    fn settings_schema(&self) -> Option<serde_json::Value> {
        None
    }

    /// Apply settings from persisted config
    fn apply_settings(&mut self, _settings: &serde_json::Value) {}

    /// Get current settings as JSON for persistence
    fn get_settings(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    // === KEYBINDS ===

    /// List of action names that can be bound to keys
    fn bindable_actions(&self) -> Vec<BindableAction> {
        vec![]
    }

    /// Execute a bound action, returns true if handled
    fn execute_action(&mut self, _action: &str) -> bool {
        false
    }

    /// Get current display text (for simple text-based tiles)
    fn get_display_text(&self) -> Option<String> {
        None
    }
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

    /// List all registered tile module IDs (sorted)
    pub fn list_tiles(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.tiles.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Update all tiles (call each frame)
    pub fn update_all(&self) {
        for tile in self.tiles.values() {
            if let Ok(mut t) = tile.write() {
                t.update();
            }
        }
    }

    /// Update all tiles with power-aware throttling
    pub fn update_all_with_power(&self, profile: crate::PowerProfile, frame_count: u64) {
        for tile in self.tiles.values() {
            if let Ok(mut t) = tile.write() {
                let should_update = match profile {
                    crate::PowerProfile::Normal => true,
                    crate::PowerProfile::LowPower => {
                        if t.prefers_gpu() {
                            frame_count % 2 == 0
                        } else {
                            true
                        }
                    }
                    crate::PowerProfile::BatteryBackground => {
                        if t.prefers_gpu() {
                            frame_count % 8 == 0
                        } else {
                            true
                        }
                    }
                };

                if should_update {
                    t.update();
                }
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
    /// Returns true if input was handled.
    pub fn render_controls(
        &self,
        module: &str,
        draw: &Draw,
        rect: Rect,
        ctx: &RenderContext,
    ) -> bool {
        if let Some(tile) = self.tiles.get(module) {
            if let Ok(t) = tile.read() {
                return t.render_controls(draw, rect, ctx);
            }
        }
        false
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

    /// Forward a keyboard event to a tile (typically when maximized).
    /// Returns true if the tile consumed the input.
    pub fn handle_key(
        &self,
        module: &str,
        key: nannou::prelude::Key,
        ctrl: bool,
        shift: bool,
    ) -> bool {
        if let Some(tile) = self.tiles.get(module) {
            if let Ok(mut t) = tile.write() {
                return t.handle_key(key, ctrl, shift);
            }
        }
        false
    }

    /// Get error from a tile
    pub fn get_error(&self, module: &str) -> Option<TileError> {
        if let Some(tile) = self.tiles.get(module) {
            if let Ok(t) = tile.read() {
                return t.get_error();
            }
        }
        None
    }

    /// Clear error on a tile
    pub fn clear_error(&self, module: &str) {
        if let Some(tile) = self.tiles.get(module) {
            if let Ok(mut t) = tile.write() {
                t.clear_error();
            }
        }
    }
}

impl Default for TileRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Render an error overlay on a tile
pub fn render_error_overlay(draw: &Draw, rect: Rect, error: &TileError) {
    let (bg_color, fg_color, icon) = match error.severity {
        ErrorSeverity::Info => (srgba(0.1, 0.2, 0.3, 0.8), srgba(0.5, 0.7, 1.0, 1.0), "ℹ"),
        ErrorSeverity::Warning => (srgba(0.3, 0.25, 0.1, 0.8), srgba(1.0, 0.8, 0.2, 1.0), "⚠"),
        ErrorSeverity::Error => (srgba(0.3, 0.1, 0.1, 0.8), srgba(1.0, 0.3, 0.3, 1.0), "✖"),
    };

    let banner_height = 24.0;
    let banner_rect = Rect::from_x_y_w_h(
        rect.x(),
        rect.bottom() + banner_height / 2.0,
        rect.w(),
        banner_height,
    );

    draw.rect()
        .xy(banner_rect.xy())
        .wh(banner_rect.wh())
        .color(bg_color);

    draw_text(
        draw,
        FontId::PlexSansBold,
        icon,
        pt2(banner_rect.left() + 12.0, banner_rect.y()),
        14.0,
        fg_color,
        TextAlignment::Left,
    );

    let msg = if error.message.len() > 40 {
        format!("{}...", &error.message[..40])
    } else {
        error.message.clone()
    };

    draw_text(
        draw,
        FontId::PlexSansRegular,
        &msg,
        pt2(banner_rect.x() + 10.0, banner_rect.y()),
        11.0,
        fg_color,
        TextAlignment::Center,
    );
}
