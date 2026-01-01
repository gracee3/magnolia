use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Feature-gated tile rendering system
#[cfg(feature = "tile-rendering")]
pub mod tile;
#[cfg(feature = "tile-rendering")]
pub use tile::{
    render_error_overlay, BindableAction, ErrorSeverity, RenderContext, TileError, TileRegistry,
    TileRenderer,
};

pub mod patch_bay;
pub use patch_bay::{PatchBay, PatchBayError};

pub mod host;
pub use host::{ModuleHandle, ModuleImpl};

pub mod runtime;
pub use runtime::RoutedSignal;
pub use runtime::{ExecutionModel, ModuleHost, ModuleRuntime, Priority};

pub mod adapters;
pub use adapters::{SinkAdapter, SourceAdapter};

pub mod ring_buffer;
pub use ring_buffer::{RingBufferReceiver, RingBufferSender, SPSCRingBuffer};

pub mod audio_frame;
pub use audio_frame::AudioFrame;

pub mod shared_data;
pub use shared_data::{AudioData, BlobData};

pub mod plugin_loader;
pub use plugin_loader::{PluginLibrary, PluginLoader};

pub mod plugin_adapter;
pub use plugin_adapter::PluginModuleAdapter;

pub mod plugin_manager;
pub use plugin_manager::PluginManager;

pub mod sandbox;
pub use sandbox::{apply_sandbox, create_plugin_sandbox};

pub mod plugin_signing;
pub use plugin_signing::PluginVerifier;

pub mod resources {
    pub mod buffer_pool;
    pub mod gpu_map;
}
pub use resources::buffer_pool::{AudioBufferPool, BlobBufferPool, BufferPool};
pub use resources::gpu_map::{GpuBufferMap, GpuResourceMap, GpuTextureMap, GpuTextureViewMap};

/// Symbolic Kamea grid size names mapped to dimensions
/// Based on traditional planetary magic squares
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KameaGrid {
    Saturn,  // 3×3
    Jupiter, // 4×4
    Mars,    // 5×5
    Sun,     // 6×6
    Venus,   // 7×7 (default)
    Mercury, // 8×8
    Moon,    // 9×9
}

impl KameaGrid {
    /// Get grid dimensions (cols, rows)
    pub fn dimensions(&self) -> (usize, usize) {
        match self {
            KameaGrid::Saturn => (3, 3),
            KameaGrid::Jupiter => (4, 4),
            KameaGrid::Mars => (5, 5),
            KameaGrid::Sun => (6, 6),
            KameaGrid::Venus => (7, 7),
            KameaGrid::Mercury => (8, 8),
            KameaGrid::Moon => (9, 9),
        }
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "saturn" | "3" | "3x3" => Some(KameaGrid::Saturn),
            "jupiter" | "4" | "4x4" => Some(KameaGrid::Jupiter),
            "mars" | "5" | "5x5" => Some(KameaGrid::Mars),
            "sun" | "6" | "6x6" => Some(KameaGrid::Sun),
            "venus" | "7" | "7x7" => Some(KameaGrid::Venus),
            "mercury" | "8" | "8x8" => Some(KameaGrid::Mercury),
            "moon" | "9" | "9x9" => Some(KameaGrid::Moon),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub enum PowerProfile {
    #[default]
    Normal,
    LowPower,
    BatteryBackground,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LayoutConfig {
    /// Symbolic Kamea grid size (optional, overrides columns/rows when set)
    /// Values: "saturn" (3×3), "jupiter" (4×4), "mars" (5×5),
    /// "sun" (6×6), "venus" (7×7), "mercury" (8×8), "moon" (9×9)
    #[serde(default)]
    pub grid: Option<String>,
    pub columns: Vec<String>, // e.g. "30%", "1fr", "200px"
    pub rows: Vec<String>,
    pub tiles: Vec<TileConfig>,
    #[serde(default)]
    pub patches: Vec<Patch>,
    #[serde(default)]
    pub is_sleeping: bool,
    #[serde(default)]
    pub power_profile: PowerProfile,
}

impl LayoutConfig {
    /// Resolve grid to column/row counts
    /// Returns (cols, rows) tuple
    pub fn resolve_grid(&self) -> (usize, usize) {
        if let Some(ref grid_name) = self.grid {
            if let Some(kamea) = KameaGrid::from_str(grid_name) {
                return kamea.dimensions();
            }
        }
        // Fallback to explicit columns/rows
        (self.columns.len().max(1), self.rows.len().max(1))
    }

    /// Generate equal-sized track definitions for symbolic grid
    pub fn generate_tracks(&self) -> (Vec<String>, Vec<String>) {
        let (cols, rows) = self.resolve_grid();
        if self.grid.is_some() {
            // Generate 1fr tracks for symbolic grid
            let col_tracks: Vec<String> = (0..cols).map(|_| "1fr".to_string()).collect();
            let row_tracks: Vec<String> = (0..rows).map(|_| "1fr".to_string()).collect();
            (col_tracks, row_tracks)
        } else {
            (self.columns.clone(), self.rows.clone())
        }
    }

    /// Resolve tile overlaps by re-packing tiles onto the grid.
    ///
    /// Goals:
    /// - No overlaps.
    /// - Keep tiles as close as possible to their current position.
    /// - Allow resizing (shrinking) and moving other tiles if needed.
    /// - If a specific tile is being edited (move/resize), prefer keeping it fixed
    ///   at its current (col,row) and only shrink it as a last resort to make the
    ///   whole layout feasible.
    ///
    /// Returns an error when it's impossible to place all tiles (e.g. more tiles
    /// than grid cells).
    pub fn resolve_conflicts(
        &mut self,
        preferred_tile_id: Option<&str>,
    ) -> std::result::Result<(), LayoutResolveError> {
        let (cols, rows) = self.resolve_grid();
        self.resolve_conflicts_within(cols, rows, preferred_tile_id)
    }

    pub fn resolve_conflicts_within(
        &mut self,
        cols: usize,
        rows: usize,
        preferred_tile_id: Option<&str>,
    ) -> std::result::Result<(), LayoutResolveError> {
        if cols == 0 || rows == 0 {
            return Err(LayoutResolveError::InvalidGrid { cols, rows });
        }
        let cell_count = cols.saturating_mul(rows);
        if self.tiles.len() > cell_count {
            return Err(LayoutResolveError::TooManyTiles {
                tiles: self.tiles.len(),
                cells: cell_count,
                cols,
                rows,
            });
        }

        // Snapshot for retry logic (we may shrink the preferred tile until feasible).
        let original_tiles = self.tiles.clone();
        let preferred_idx =
            preferred_tile_id.and_then(|id| self.tiles.iter().position(|t| t.id == id));

        let mut preferred_bounds = preferred_idx.map(|idx| {
            let t = &self.tiles[idx];
            let col = t.col.min(cols.saturating_sub(1));
            let row = t.row.min(rows.saturating_sub(1));
            let mut w = t.colspan.unwrap_or(1).max(1);
            let mut h = t.rowspan.unwrap_or(1).max(1);
            w = w.min(cols.saturating_sub(col).max(1));
            h = h.min(rows.saturating_sub(row).max(1));
            (idx, col, row, w, h)
        });

        let max_retry_steps = cols.saturating_mul(rows).max(1);
        for _ in 0..max_retry_steps {
            // Reset tiles to original snapshot before attempting a full solve.
            self.tiles = original_tiles.clone();
            if let Some((idx, col, row, w, h)) = preferred_bounds {
                let t = &mut self.tiles[idx];
                t.col = col;
                t.row = row;
                t.colspan = Some(w);
                t.rowspan = Some(h);
            }

            match self.try_pack_no_overlap(cols, rows, preferred_idx) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    // If we have a preferred tile, shrink it as a last resort and retry.
                    if let Some((idx, col, row, mut w, mut h)) = preferred_bounds {
                        if w == 1 && h == 1 {
                            return Err(e);
                        }
                        // Shrink the larger dimension first.
                        if w >= h {
                            w = w.saturating_sub(1).max(1);
                        } else {
                            h = h.saturating_sub(1).max(1);
                        }
                        preferred_bounds = Some((idx, col, row, w, h));
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        Err(LayoutResolveError::RetryLimit)
    }

    fn try_pack_no_overlap(
        &mut self,
        cols: usize,
        rows: usize,
        preferred_idx: Option<usize>,
    ) -> std::result::Result<(), LayoutResolveError> {
        let mut occupancy = vec![false; cols.saturating_mul(rows)];

        let mut order: Vec<usize> = (0..self.tiles.len()).collect();
        if let Some(p) = preferred_idx {
            order.retain(|&i| i != p);
            order.insert(0, p);
        }

        for idx in order {
            let is_preferred = preferred_idx == Some(idx);
            let (placed_col, placed_row, placed_w, placed_h) = {
                let t = &self.tiles[idx];
                let desired_col = t.col.min(cols.saturating_sub(1));
                let desired_row = t.row.min(rows.saturating_sub(1));
                let mut w = t.colspan.unwrap_or(1).max(1);
                let mut h = t.rowspan.unwrap_or(1).max(1);
                w = w.min(cols.saturating_sub(desired_col).max(1));
                h = h.min(rows.saturating_sub(desired_row).max(1));

                if is_preferred {
                    // Keep preferred tile fixed (no movement), only shrink as needed for bounds.
                    if !rect_is_free(&occupancy, cols, desired_col, desired_row, w, h) {
                        return Err(LayoutResolveError::CannotPlace {
                            tile_id: t.id.clone(),
                            cols,
                            rows,
                        });
                    }
                    (desired_col, desired_row, w, h)
                } else {
                    place_tile_best_effort(&occupancy, cols, rows, desired_col, desired_row, w, h)
                        .ok_or_else(|| LayoutResolveError::CannotPlace {
                        tile_id: t.id.clone(),
                        cols,
                        rows,
                    })?
                }
            };

            {
                let t = &mut self.tiles[idx];
                t.col = placed_col;
                t.row = placed_row;
                t.colspan = Some(placed_w);
                t.rowspan = Some(placed_h);
            }
            rect_occupy(
                &mut occupancy,
                cols,
                placed_col,
                placed_row,
                placed_w,
                placed_h,
            );
        }

        Ok(())
    }
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum LayoutResolveError {
    #[error("invalid grid size cols={cols} rows={rows}")]
    InvalidGrid { cols: usize, rows: usize },

    #[error("too many tiles for grid ({tiles} tiles > {cells} cells) in {cols}x{rows}")]
    TooManyTiles {
        tiles: usize,
        cells: usize,
        cols: usize,
        rows: usize,
    },

    #[error("could not place tile '{tile_id}' without overlap in {cols}x{rows}")]
    CannotPlace {
        tile_id: String,
        cols: usize,
        rows: usize,
    },

    #[error("layout resolver exceeded retry limit")]
    RetryLimit,
}

fn rect_is_free(
    occupancy: &[bool],
    cols: usize,
    col: usize,
    row: usize,
    w: usize,
    h: usize,
) -> bool {
    for r in row..row + h {
        for c in col..col + w {
            let idx = r.saturating_mul(cols).saturating_add(c);
            if occupancy.get(idx).copied().unwrap_or(true) {
                return false;
            }
        }
    }
    true
}

fn rect_occupy(occupancy: &mut [bool], cols: usize, col: usize, row: usize, w: usize, h: usize) {
    for r in row..row + h {
        for c in col..col + w {
            let idx = r.saturating_mul(cols).saturating_add(c);
            if let Some(cell) = occupancy.get_mut(idx) {
                *cell = true;
            }
        }
    }
}

fn place_tile_best_effort(
    occupancy: &[bool],
    cols: usize,
    rows: usize,
    desired_col: usize,
    desired_row: usize,
    start_w: usize,
    start_h: usize,
) -> Option<(usize, usize, usize, usize)> {
    let mut w = start_w.max(1);
    let mut h = start_h.max(1);

    // Precompute candidate origins sorted by Manhattan distance from desired.
    let mut origins: Vec<(usize, usize)> = Vec::with_capacity(cols.saturating_mul(rows));
    for r in 0..rows {
        for c in 0..cols {
            origins.push((c, r));
        }
    }
    origins.sort_by(|(ac, ar), (bc, br)| {
        let da = ac.abs_diff(desired_col) + ar.abs_diff(desired_row);
        let db = bc.abs_diff(desired_col) + br.abs_diff(desired_row);
        da.cmp(&db)
            .then_with(|| ar.cmp(br))
            .then_with(|| ac.cmp(bc))
    });

    while w >= 1 && h >= 1 {
        // 1) Try the desired origin first.
        if desired_col + w <= cols && desired_row + h <= rows {
            if rect_is_free(occupancy, cols, desired_col, desired_row, w, h) {
                return Some((desired_col, desired_row, w, h));
            }
        }

        // 2) Find the closest origin that fits.
        for (c, r) in &origins {
            if *c + w <= cols && *r + h <= rows {
                if rect_is_free(occupancy, cols, *c, *r, w, h) {
                    return Some((*c, *r, w, h));
                }
            }
        }

        // 3) Shrink and retry.
        if w == 1 && h == 1 {
            break;
        }
        if w >= h {
            w = w.saturating_sub(1).max(1);
        } else {
            h = h.saturating_sub(1).max(1);
        }
    }

    None
}

/// Per-tile instance settings stored in layout config
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TileSettings {
    /// Module-specific settings as JSON
    #[serde(default)]
    pub config: serde_json::Value,

    /// Keybindings: action name -> key (e.g., "mute" -> "m")
    #[serde(default)]
    pub keybinds: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TileConfig {
    pub id: String,
    pub col: usize,
    pub row: usize,
    pub colspan: Option<usize>,
    pub rowspan: Option<usize>,
    pub module: String, // e.g. "editor", "word_count"
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Per-tile instance settings (module interprets these)
    #[serde(default)]
    pub settings: TileSettings,
}

fn default_enabled() -> bool {
    true
}

use schemars::JsonSchema;
use std::fmt::Debug;

pub type Result<T> = std::result::Result<T, anyhow::Error>;

// Re-export core types from signals
pub use magnolia_signals::{AstrologyData, ControlSignal, DataType, PortDirection, Signal};
pub use magnolia_signals::{AudioBufferHandle, BlobHandle, GpuBufferHandle, GpuTextureHandle};

/// A typed port on a module for connecting to other modules
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Port {
    /// Unique identifier within the module
    pub id: String,
    /// Human-readable label
    pub label: String,
    /// Type of data this port handles
    pub data_type: DataType,
    /// Whether this port receives (Input) or emits (Output) data
    pub direction: PortDirection,
}

/// Schema describing a module's capabilities and interface
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModuleSchema {
    /// Unique module identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of what the module does
    pub description: String,
    /// Available input/output ports
    pub ports: Vec<Port>,
    /// Optional JSON Schema for settings UI
    pub settings_schema: Option<serde_json::Value>,
}

/// A connection between two ports on different modules
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Patch {
    /// Unique patch identifier
    pub id: String,
    /// Source module ID
    pub source_module: String,
    /// Source port ID (must be Output direction)
    pub source_port: String,
    /// Sink module ID
    pub sink_module: String,
    /// Sink port ID (must be Input direction)
    pub sink_port: String,
}

// Signal types replaced by magnolia_signals re-export

// ============================================================================
// MODULE TRAITS
// ============================================================================

/// A Source emits Signals into the Patch Bay.
///
/// Examples: Clipboard monitor, Keyboard listener, Timer, HTTP Server, Astrology Clock.
#[async_trait]
pub trait Source: Send + Sync {
    /// The name of this source (e.g., "clipboard_monitor")
    fn name(&self) -> &str;

    /// Returns the schema describing this module's ports and capabilities
    fn schema(&self) -> ModuleSchema;

    /// Whether this module is currently enabled
    fn is_enabled(&self) -> bool {
        true
    }

    /// Enable or disable this module
    fn set_enabled(&mut self, enabled: bool);

    /// Wait for the next signal from this source.
    /// Returns `None` if the source is exhausted/closed.
    async fn poll(&mut self) -> Option<Signal>;
}

/// A Sink consumes Signals from the Patch Bay.
///
/// Examples: Log file, TTS Speaker, Sigil Renderer, HTTP Client, Screen Display.
#[async_trait]
pub trait Sink: Send + Sync {
    /// The name of this sink
    fn name(&self) -> &str;

    /// Returns the schema describing this module's ports and capabilities
    fn schema(&self) -> ModuleSchema;

    /// Whether this module is currently enabled
    fn is_enabled(&self) -> bool {
        true
    }

    /// Enable or disable this module
    fn set_enabled(&mut self, enabled: bool);

    /// Render the current output state as a string for clipboard copy
    fn render_output(&self) -> Option<String> {
        None
    }

    /// Consume a signal and optionally produce an output signal.
    ///
    /// Returns:
    /// - `Ok(Some(signal))` - Successfully processed and produced an output signal
    /// - `Ok(None)` - Successfully processed, no output to emit
    /// - `Err(e)` - Processing failed
    ///
    /// This replaces the previous pattern of passing a sender to the sink,
    /// allowing cleaner back-channel communication through the return value.
    async fn consume(&self, signal: Signal) -> Result<Option<Signal>>;
}

/// A Processor is both a Source and Sink - it transforms signals (middleware).
///
/// Examples: Text sanitizer, Format converter, Rate limiter, Aggregator.
#[async_trait]
pub trait Processor: Send + Sync {
    /// The name of this processor
    fn name(&self) -> &str;

    /// Returns the schema describing this module's ports and capabilities
    fn schema(&self) -> ModuleSchema;

    /// Whether this module is currently enabled
    fn is_enabled(&self) -> bool {
        true
    }

    /// Enable or disable this module
    fn set_enabled(&mut self, enabled: bool);

    /// Process an input signal and optionally emit an output signal
    async fn process(&mut self, signal: Signal) -> Result<Option<Signal>>;
}

/// A Transform modifies a Signal in flight (synchronous version).
/// (Optional advanced feature for later, but good to have the trait)
#[async_trait]
pub trait Transform: Send + Sync {
    async fn apply(&self, signal: Signal) -> Result<Signal>;
}
