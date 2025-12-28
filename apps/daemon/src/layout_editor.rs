use nannou::prelude::*;
use talisman_core::{LayoutConfig, TileConfig, Patch, ModuleSchema, DataType, PortDirection};
use std::collections::HashSet;

/// Direction for keyboard navigation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Role in patching workflow
#[derive(Debug, Clone, PartialEq)]
pub enum PatchRole {
    /// Showing Source/Sink buttons, awaiting selection
    SelectingRole,
    /// User clicked Source, now selecting a sink tile
    Source,
    /// User clicked Sink, now selecting a source tile
    Sink,
}

/// Edit mode sub-states
#[derive(Debug, Clone, PartialEq)]
pub enum EditState {
    /// Navigating grid with cursor (default state in edit mode)
    Navigation,
    /// A tile is selected, can move/resize via keyboard or enter patch mode
    TileSelected { tile_id: String },
    /// Move/Resize mode - tile shown as 1×1, awaiting clicks to define new bounds
    MoveResize {
        tile_id: String,
        /// Original bounds for revert on ESC (col, row, colspan, rowspan)
        original_bounds: (usize, usize, usize, usize),
        /// If Some, first click was made (new start position)
        /// If None, tile is at original start, next click sets end
        start_cell: Option<(usize, usize)>,
    },
    /// Patching mode - selecting source or sink connection
    Patching {
        tile_id: String,
        role: PatchRole,
    },
}

impl Default for EditState {
    fn default() -> Self {
        EditState::Navigation
    }
}


/// Main layout editor state - keyboard-driven, no drag operations
#[derive(Debug)]
pub struct LayoutEditor {
    /// Whether edit mode is active
    pub edit_mode: bool,
    /// Current sub-state within edit mode
    pub edit_state: EditState,
    /// Current keyboard cursor position (col, row)
    pub cursor_cell: (usize, usize),
    /// Whether to display the grid overlay
    pub show_grid_overlay: bool,
    /// Dirty flag - true if there are unsaved changes
    pub pending_changes: bool,
    /// Cached list of available modules for placement
    pub available_modules: Vec<ModuleSchema>,
    /// Hover cell for visual feedback
    pub hover_cell: Option<(usize, usize)>,
}

impl LayoutEditor {
    pub fn new() -> Self {
        Self {
            edit_mode: false,
            edit_state: EditState::Navigation,
            cursor_cell: (0, 0),
            show_grid_overlay: true,
            pending_changes: false,
            available_modules: Vec::new(),
            hover_cell: None,
        }
    }

    /// Toggle edit mode on/off
    pub fn toggle_edit_mode(&mut self) -> bool {
        self.edit_mode = !self.edit_mode;
        if self.edit_mode {
            // Entering edit mode
            self.edit_state = EditState::Navigation;
            self.show_grid_overlay = true;
        } else {
            // Exiting edit mode - clear all state
            self.edit_state = EditState::Navigation;
            self.hover_cell = None;
        }
        // Return whether we should save (exiting edit mode with pending changes)
        !self.edit_mode && self.pending_changes
    }

    /// Navigate cursor in the specified direction
    pub fn navigate_cursor(&mut self, direction: Direction, grid_cols: usize, grid_rows: usize) {
        let (col, row) = self.cursor_cell;
        self.cursor_cell = match direction {
            Direction::Up => (col, row.saturating_sub(1)),
            Direction::Down => (col, (row + 1).min(grid_rows.saturating_sub(1))),
            Direction::Left => (col.saturating_sub(1), row),
            Direction::Right => ((col + 1).min(grid_cols.saturating_sub(1)), row),
        };
    }

    /// Select the tile at the current cursor position, or empty cell action
    pub fn select_at_cursor(&mut self, layout: &LayoutConfig) -> Option<String> {
        let (col, row) = self.cursor_cell;
        if let Some(tile) = Self::get_tile_at_cell(layout, col, row) {
            self.edit_state = EditState::TileSelected {
                tile_id: tile.id.clone(),
            };
            Some(tile.id.clone())
        } else {
            // Empty cell - could trigger module picker
            None
        }
    }

    /// Select a specific tile by ID
    pub fn select_tile(&mut self, tile_id: String) {
        self.edit_state = EditState::TileSelected { tile_id };
    }

    /// Deselect and return to navigation
    pub fn deselect(&mut self) {
        self.edit_state = EditState::Navigation;
    }

    /// Enter move/resize mode for the selected tile
    /// Tile will be shown as 1×1 at its current start position
    pub fn enter_move_resize_mode(&mut self, layout: &LayoutConfig) {
        if let EditState::TileSelected { tile_id } = &self.edit_state {
            // Find tile to get its current bounds
            if let Some(tile) = layout.tiles.iter().find(|t| t.id == *tile_id) {
                let original_bounds = (
                    tile.col,
                    tile.row,
                    tile.colspan.unwrap_or(1),
                    tile.rowspan.unwrap_or(1),
                );
                self.edit_state = EditState::MoveResize {
                    tile_id: tile_id.clone(),
                    original_bounds,
                    start_cell: None,
                };
            }
        }
    }

    /// Handle a click during move/resize mode
    /// Returns true if operation is complete
    pub fn handle_move_resize_click(
        &mut self,
        cell: (usize, usize),
        layout: &mut LayoutConfig,
    ) -> bool {
        if let EditState::MoveResize { tile_id, original_bounds, start_cell } = &self.edit_state {
            let tile_id = tile_id.clone();
            let original_bounds = *original_bounds;
            
            if start_cell.is_none() {
                // First click: This could be either:
                // 1. Setting end position (resize from original start)
                // 2. Setting new start position (user wants to move)
                
                // If clicking on the same start cell as original, interpret as "set new start"
                if cell == (original_bounds.0, original_bounds.1) {
                    // User clicked on current start - they want to define new start next
                    self.edit_state = EditState::MoveResize {
                        tile_id,
                        original_bounds,
                        start_cell: Some(cell),
                    };
                    return false;
                }
                
                // Otherwise, this is the end position (resize from original start)
                let start = (original_bounds.0, original_bounds.1);
                let end = cell;
                
                let new_col = start.0.min(end.0);
                let new_row = start.1.min(end.1);
                let colspan = (start.0.max(end.0) - new_col + 1).max(1);
                let rowspan = (start.1.max(end.1) - new_row + 1).max(1);
                
                if self.is_placement_valid(layout, &tile_id, new_col, new_row, colspan, rowspan) {
                    if let Some(tile) = layout.tiles.iter_mut().find(|t| t.id == tile_id) {
                        tile.col = new_col;
                        tile.row = new_row;
                        tile.colspan = Some(colspan);
                        tile.rowspan = Some(rowspan);
                        self.pending_changes = true;
                    }
                    self.edit_state = EditState::TileSelected { tile_id };
                    return true;
                }
                false
            } else {
                // Second click: set end position (moved from start_cell)
                let start = start_cell.unwrap();
                let end = cell;
                
                let new_col = start.0.min(end.0);
                let new_row = start.1.min(end.1);
                let colspan = (start.0.max(end.0) - new_col + 1).max(1);
                let rowspan = (start.1.max(end.1) - new_row + 1).max(1);
                
                if self.is_placement_valid(layout, &tile_id, new_col, new_row, colspan, rowspan) {
                    if let Some(tile) = layout.tiles.iter_mut().find(|t| t.id == tile_id) {
                        tile.col = new_col;
                        tile.row = new_row;
                        tile.colspan = Some(colspan);
                        tile.rowspan = Some(rowspan);
                        self.pending_changes = true;
                    }
                    self.edit_state = EditState::TileSelected { tile_id };
                    return true;
                }
                false
            }
        } else {
            false
        }
    }

    /// Revert tile to original bounds (on ESC during MoveResize)
    pub fn revert_move_resize(&mut self, layout: &mut LayoutConfig) {
        if let EditState::MoveResize { tile_id, original_bounds, .. } = &self.edit_state {
            if let Some(tile) = layout.tiles.iter_mut().find(|t| t.id == *tile_id) {
                tile.col = original_bounds.0;
                tile.row = original_bounds.1;
                tile.colspan = Some(original_bounds.2);
                tile.rowspan = Some(original_bounds.3);
            }
            self.edit_state = EditState::TileSelected { tile_id: tile_id.clone() };
        }
    }


    /// Enter patch mode for the selected tile
    pub fn enter_patch_mode(&mut self) {
        if let EditState::TileSelected { tile_id } = &self.edit_state {
            self.edit_state = EditState::Patching {
                tile_id: tile_id.clone(),
                role: PatchRole::SelectingRole,
            };
        }
    }

    /// Select patch role (Source or Sink)
    pub fn select_patch_role(&mut self, role: PatchRole) {
        if let EditState::Patching { tile_id, .. } = &self.edit_state {
            self.edit_state = EditState::Patching {
                tile_id: tile_id.clone(),
                role,
            };
        }
    }

    /// Complete a patch to the target tile
    /// Returns the new Patch if successful
    pub fn complete_patch(
        &mut self,
        target_tile_id: &str,
        layout: &LayoutConfig,
        get_module_for_tile: impl Fn(&str) -> Option<String>,
    ) -> Option<Patch> {
        if let EditState::Patching { tile_id, role } = &self.edit_state {
            let tile_id = tile_id.clone();
            let role = role.clone();
            
            // Get module IDs for both tiles
            let source_module = match &role {
                PatchRole::Source => get_module_for_tile(&tile_id),
                PatchRole::Sink => get_module_for_tile(target_tile_id),
                PatchRole::SelectingRole => return None,
            };
            
            let sink_module = match &role {
                PatchRole::Source => get_module_for_tile(target_tile_id),
                PatchRole::Sink => get_module_for_tile(&tile_id),
                PatchRole::SelectingRole => return None,
            };
            
            if let (Some(src), Some(snk)) = (source_module, sink_module) {
                // Return to tile selected state
                self.edit_state = EditState::TileSelected { tile_id };
                self.pending_changes = true;
                
                // Create a new patch (caller will need to determine port names)
                return Some(Patch {
                    id: format!("patch_{}_{}", src, snk),
                    source_module: src,
                    source_port: "default_out".to_string(), // Placeholder
                    sink_module: snk,
                    sink_port: "default_in".to_string(), // Placeholder
                });
            }
        }
        None
    }

    /// Cancel current operation and return to appropriate state
    pub fn cancel_operation(&mut self) {
        self.edit_state = match &self.edit_state {
            EditState::MoveResize { tile_id, .. } => EditState::TileSelected {
                tile_id: tile_id.clone(),
            },
            EditState::Patching { tile_id, role } => {
                if *role == PatchRole::SelectingRole {
                    EditState::TileSelected {
                        tile_id: tile_id.clone(),
                    }
                } else {
                    EditState::Patching {
                        tile_id: tile_id.clone(),
                        role: PatchRole::SelectingRole,
                    }
                }
            }
            EditState::TileSelected { .. } => EditState::Navigation,
            EditState::Navigation => EditState::Navigation,
        };
    }


    /// Check if a placement is valid (no overlaps with other tiles)
    pub fn is_placement_valid(
        &self,
        layout: &LayoutConfig,
        exclude_tile_id: &str,
        col: usize,
        row: usize,
        colspan: usize,
        rowspan: usize,
    ) -> bool {
        for tile in &layout.tiles {
            if tile.id == exclude_tile_id {
                continue;
            }
            
            let t_col = tile.col;
            let t_row = tile.row;
            let t_colspan = tile.colspan.unwrap_or(1);
            let t_rowspan = tile.rowspan.unwrap_or(1);
            
            // Check for overlap
            let overlap_x = col < t_col + t_colspan && col + colspan > t_col;
            let overlap_y = row < t_row + t_rowspan && row + rowspan > t_row;
            
            if overlap_x && overlap_y {
                return false;
            }
        }
        true
    }

    /// Check if a single cell is occupied by any tile
    pub fn is_cell_occupied(&self, layout: &LayoutConfig, col: usize, row: usize) -> Option<String> {
        Self::get_tile_at_cell(layout, col, row).map(|t| t.id.clone())
    }

    /// Get tile at a specific cell
    pub fn get_tile_at_cell(layout: &LayoutConfig, col: usize, row: usize) -> Option<&TileConfig> {
        for tile in &layout.tiles {
            let t_col = tile.col;
            let t_row = tile.row;
            let t_colspan = tile.colspan.unwrap_or(1);
            let t_rowspan = tile.rowspan.unwrap_or(1);
            
            if col >= t_col && col < t_col + t_colspan && row >= t_row && row < t_row + t_rowspan {
                return Some(tile);
            }
        }
        None
    }

    /// Get grid cell at mouse position
    pub fn get_grid_cell(
        &self,
        mouse_pos: Vec2,
        win_rect: Rect,
        col_sizes: &[f32],
        row_sizes: &[f32],
    ) -> Option<(usize, usize)> {
        if !win_rect.contains(mouse_pos) {
            return None;
        }

        // Calculate which column
        let mut x_accum = win_rect.left();
        let mut col = None;
        for (i, &width) in col_sizes.iter().enumerate() {
            if mouse_pos.x >= x_accum && mouse_pos.x < x_accum + width {
                col = Some(i);
                break;
            }
            x_accum += width;
        }

        // Calculate which row
        let mut y_accum = win_rect.top();
        let mut row = None;
        for (i, &height) in row_sizes.iter().enumerate() {
            if mouse_pos.y <= y_accum && mouse_pos.y > y_accum - height {
                row = Some(i);
                break;
            }
            y_accum -= height;
        }

        match (col, row) {
            (Some(c), Some(r)) => Some((c, r)),
            _ => None,
        }
    }

    /// Get the currently selected tile ID
    pub fn selected_tile_id(&self) -> Option<&str> {
        match &self.edit_state {
            EditState::TileSelected { tile_id } => Some(tile_id),
            EditState::MoveResize { tile_id, .. } => Some(tile_id),
            EditState::Patching { tile_id, .. } => Some(tile_id),
            EditState::Navigation => None,
        }
    }
}


// =============================================================================
// COLOR HELPERS
// =============================================================================

/// Generate a deterministic color from a tile ID string
/// Uses HSL color space for pleasant, distinct hues
pub fn tile_color(id: &str) -> Srgba<u8> {
    // Hash the string to get a deterministic value
    let hash = id.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    
    // Use golden ratio angle for hue distribution (avoids clustering)
    let hue = ((hash % 360) as f32) / 360.0;
    let saturation = 0.7;
    let lightness = 0.55;
    
    // Convert HSL to RGB
    let c = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = c * (1.0 - ((hue * 6.0) % 2.0 - 1.0).abs());
    let m = lightness - c / 2.0;
    
    let (r, g, b) = match (hue * 6.0) as u8 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    
    Srgba::new(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
        255
    )
}

/// Color for each DataType (for port visualization)
pub fn data_type_color(dt: &DataType) -> Srgba<u8> {
    match dt {
        DataType::Text => Srgba::new(0, 255, 128, 255),        // Green
        DataType::Audio => Srgba::new(255, 128, 0, 255),       // Orange
        DataType::Astrology => Srgba::new(180, 100, 255, 255), // Purple
        DataType::Blob => Srgba::new(100, 150, 255, 255),      // Blue
        DataType::Control => Srgba::new(255, 80, 80, 255),     // Red
        DataType::Video => Srgba::new(255, 200, 50, 255),      // Yellow
        DataType::Network => Srgba::new(50, 200, 200, 255),    // Teal
        DataType::Numeric => Srgba::new(200, 200, 100, 255),   // Olive/Yellow
        DataType::Any => Srgba::new(255, 255, 255, 255),       // White
    }
}


// =============================================================================
// RENDERING FUNCTIONS
// =============================================================================


/// Render the edit mode grid overlay
pub fn render_edit_overlay(
    draw: &Draw,
    win_rect: Rect,
    col_sizes: &[f32],
    row_sizes: &[f32],
) {
    let grid_color = rgba(1.0, 1.0, 1.0, 0.15);

    // Vertical column dividers
    let mut x = win_rect.left();
    for &width in col_sizes {
        x += width;
        draw.line()
            .start(pt2(x, win_rect.bottom()))
            .end(pt2(x, win_rect.top()))
            .color(grid_color)
            .stroke_weight(1.0);
    }

    // Horizontal row dividers
    let mut y = win_rect.top();
    for &height in row_sizes {
        y -= height;
        draw.line()
            .start(pt2(win_rect.left(), y))
            .end(pt2(win_rect.right(), y))
            .color(grid_color)
            .stroke_weight(1.0);
    }
}

/// Render cell indicators (cursor, validity, etc.)
pub fn render_cell_indicators(
    draw: &Draw,
    win_rect: Rect,
    col_sizes: &[f32],
    row_sizes: &[f32],
    layout: &LayoutConfig,
    editor: &LayoutEditor,
) {
    let grid_cols = col_sizes.len();
    let grid_rows = row_sizes.len();

    for col in 0..grid_cols {
        for row in 0..grid_rows {
            let rect = calculate_cell_rect(win_rect, col_sizes, row_sizes, col, row);
            let is_cursor = editor.cursor_cell == (col, row);
            let is_occupied = LayoutEditor::get_tile_at_cell(layout, col, row).is_some();
            let is_hover = editor.hover_cell == Some((col, row));

            // Determine cell state for coloring
            match &editor.edit_state {
                EditState::Navigation => {
                    if is_cursor {
                        // Cyan outline for keyboard cursor
                        draw.rect()
                            .xy(rect.xy())
                            .wh(rect.wh())
                            .no_fill()
                            .stroke(rgba(0.0, 1.0, 1.0, 0.8))
                            .stroke_weight(2.0);
                    }
                }
                EditState::TileSelected { tile_id } => {
                    if is_cursor {
                        draw.rect()
                            .xy(rect.xy())
                            .wh(rect.wh())
                            .no_fill()
                            .stroke(rgba(0.0, 1.0, 1.0, 0.6))
                            .stroke_weight(2.0);
                    }
                }
                EditState::MoveResize { start_cell, original_bounds, tile_id } => {
                    // Show validity indicators
                    if is_occupied {
                        // Red for occupied cells (unless it's the tile being moved)
                        let is_self = LayoutEditor::get_tile_at_cell(layout, col, row)
                            .map(|t| t.id == *tile_id)
                            .unwrap_or(false);
                        if !is_self {
                            draw.rect()
                                .xy(rect.xy())
                                .wh(rect.wh())
                                .color(rgba(1.0, 0.2, 0.2, 0.2));
                        }
                    } else {
                        // Green for available cells
                        draw.rect()
                            .xy(rect.xy())
                            .wh(rect.wh())
                            .color(rgba(0.2, 1.0, 0.2, 0.15));
                    }

                    // Highlight current start position (1×1 anchor)
                    let current_start = start_cell.unwrap_or((original_bounds.0, original_bounds.1));
                    if (col, row) == current_start {
                        draw.rect()
                            .xy(rect.xy())
                            .wh(rect.wh())
                            .no_fill()
                            .stroke(rgba(0.0, 1.0, 0.5, 1.0))
                            .stroke_weight(4.0);
                    }

                    if is_cursor || is_hover {
                        draw.rect()
                            .xy(rect.xy())
                            .wh(rect.wh())
                            .no_fill()
                            .stroke(rgba(1.0, 1.0, 0.0, 0.8))
                            .stroke_weight(2.0);
                    }
                }

                EditState::Patching { .. } => {
                    if is_cursor || is_hover {
                        // Will be colored green/red based on compatibility
                        // For now, just highlight
                        draw.rect()
                            .xy(rect.xy())
                            .wh(rect.wh())
                            .no_fill()
                            .stroke(rgba(1.0, 0.8, 0.0, 0.8))
                            .stroke_weight(2.0);
                    }
                }
            }
        }
    }
}

/// Render module labels on tiles in edit mode
pub fn render_tile_labels(
    draw: &Draw,
    win_rect: Rect,
    layout: &LayoutConfig,
    col_sizes: &[f32],
    row_sizes: &[f32],
    selected_tile_id: Option<&str>,
) {
    for tile in &layout.tiles {
        if let Some(rect) = calculate_tile_rect(win_rect, col_sizes, row_sizes, tile) {
            let is_selected = selected_tile_id == Some(&tile.id);
            
            // Get deterministic color for this tile
            let tile_col = tile_color(&tile.id);
            
            // Draw thick colored border for visibility
            draw.rect()
                .xy(rect.xy())
                .wh(rect.wh())
                .no_fill()
                .stroke(tile_col)
                .stroke_weight(4.0);
            
            // Additional selection highlight (cyan glow)
            if is_selected {
                draw.rect()
                    .xy(rect.xy())
                    .wh(pt2(rect.w() - 4.0, rect.h() - 4.0))
                    .no_fill()
                    .stroke(rgba(0.0, 1.0, 1.0, 1.0))
                    .stroke_weight(2.0);
            }

            // Module name label
            let label_color = if is_selected {
                rgba(0.0, 1.0, 1.0, 1.0)
            } else {
                rgba(0.9, 0.9, 0.9, 0.9)
            };

            // Module name at top-left of tile (larger font)
            draw.text(&tile.module)
                .xy(pt2(rect.left() + 10.0, rect.top() - 14.0))
                .color(label_color)
                .font_size(14)
                .left_justify();

            // Tile ID below module name
            draw.text(&format!("[{}]", tile.id))
                .xy(pt2(rect.left() + 10.0, rect.top() - 30.0))
                .color(rgba(0.6, 0.6, 0.6, 0.8))
                .font_size(11)
                .left_justify();

            // Position and size info at bottom-right
            let pos_info = format!(
                "({},{}) {}×{}",
                tile.col,
                tile.row,
                tile.colspan.unwrap_or(1),
                tile.rowspan.unwrap_or(1)
            );
            draw.text(&pos_info)
                .xy(pt2(rect.right() - 10.0, rect.bottom() + 14.0))
                .color(rgba(0.6, 0.6, 0.6, 0.7))
                .font_size(10)
                .right_justify();
        }
    }
}


/// Render patch cables as bezier curves between connected tiles
pub fn render_patch_cables(
    draw: &Draw,
    win_rect: Rect,
    patches: &[Patch],
    layout: &LayoutConfig,
    col_sizes: &[f32],
    row_sizes: &[f32],
    tile_to_module: impl Fn(&str) -> String,
) {
    for patch in patches {
        // Find source and sink tiles
        let source_tile = layout.tiles.iter().find(|t| {
            tile_to_module(&t.id) == patch.source_module || t.module == patch.source_module
        });
        let sink_tile = layout.tiles.iter().find(|t| {
            tile_to_module(&t.id) == patch.sink_module || t.module == patch.sink_module
        });

        if let (Some(src), Some(snk)) = (source_tile, sink_tile) {
            if let (Some(src_rect), Some(snk_rect)) = (
                calculate_tile_rect(win_rect, col_sizes, row_sizes, src),
                calculate_tile_rect(win_rect, col_sizes, row_sizes, snk),
            ) {
                // Draw bezier curve from source right edge to sink left edge
                let start = pt2(src_rect.right(), src_rect.y());
                let end = pt2(snk_rect.left(), snk_rect.y());
                let ctrl_offset = (end.x - start.x).abs() * 0.4;

                let ctrl1 = pt2(start.x + ctrl_offset, start.y);
                let ctrl2 = pt2(end.x - ctrl_offset, end.y);

                // Draw cable
                let cable_color = rgba(0.3, 0.8, 0.3, 0.6);
                
                // Approximate bezier with line segments
                let segments = 20;
                for i in 0..segments {
                    let t0 = i as f32 / segments as f32;
                    let t1 = (i + 1) as f32 / segments as f32;
                    
                    let p0 = bezier_point(start, ctrl1, ctrl2, end, t0);
                    let p1 = bezier_point(start, ctrl1, ctrl2, end, t1);
                    
                    draw.line()
                        .start(p0)
                        .end(p1)
                        .color(cable_color)
                        .stroke_weight(2.0);
                }

                // Arrow head at end
                let arrow_size = 8.0;
                let angle = (end.y - ctrl2.y).atan2(end.x - ctrl2.x);
                draw.tri()
                    .points(
                        end,
                        pt2(
                            end.x - arrow_size * (angle + 0.4).cos(),
                            end.y - arrow_size * (angle + 0.4).sin(),
                        ),
                        pt2(
                            end.x - arrow_size * (angle - 0.4).cos(),
                            end.y - arrow_size * (angle - 0.4).sin(),
                        ),
                    )
                    .color(cable_color);
            }
        }
    }
}

/// Render patch mode UI (Source/Sink buttons would be drawn via Egui, this handles hover states)
pub fn render_patch_hover(
    draw: &Draw,
    win_rect: Rect,
    col_sizes: &[f32],
    row_sizes: &[f32],
    layout: &LayoutConfig,
    hover_tile_id: Option<&str>,
    source_tile_id: &str,
    is_compatible: bool,
) {
    if let Some(hover_id) = hover_tile_id {
        if hover_id != source_tile_id {
            if let Some(tile) = layout.tiles.iter().find(|t| t.id == hover_id) {
                if let Some(rect) = calculate_tile_rect(win_rect, col_sizes, row_sizes, tile) {
                    let color = if is_compatible {
                        rgba(0.2, 1.0, 0.2, 0.3) // Green for compatible
                    } else {
                        rgba(1.0, 0.2, 0.2, 0.3) // Red for incompatible
                    };
                    
                    draw.rect()
                        .xy(rect.xy())
                        .wh(rect.wh())
                        .color(color);
                }
            }
        }
    }
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Calculate the screen rect for a single grid cell
fn calculate_cell_rect(
    win_rect: Rect,
    col_sizes: &[f32],
    row_sizes: &[f32],
    col: usize,
    row: usize,
) -> Rect {
    let start_x: f32 = col_sizes.iter().take(col).sum();
    let start_y: f32 = row_sizes.iter().take(row).sum();
    let width = col_sizes.get(col).copied().unwrap_or(0.0);
    let height = row_sizes.get(row).copied().unwrap_or(0.0);

    let cx = win_rect.left() + start_x + width / 2.0;
    let cy = win_rect.top() - start_y - height / 2.0;

    Rect::from_x_y_w_h(cx, cy, width, height)
}

/// Calculate the screen rect for a tile
fn calculate_tile_rect(
    win_rect: Rect,
    col_sizes: &[f32],
    row_sizes: &[f32],
    tile: &TileConfig,
) -> Option<Rect> {
    let start_x: f32 = col_sizes.iter().take(tile.col).sum();
    let width: f32 = col_sizes
        .iter()
        .skip(tile.col)
        .take(tile.colspan.unwrap_or(1))
        .sum();

    let start_y: f32 = row_sizes.iter().take(tile.row).sum();
    let height: f32 = row_sizes
        .iter()
        .skip(tile.row)
        .take(tile.rowspan.unwrap_or(1))
        .sum();

    let cx = win_rect.left() + start_x + width / 2.0;
    let cy = win_rect.top() - start_y - height / 2.0;

    Some(Rect::from_x_y_w_h(cx, cy, width, height))
}

/// Cubic bezier point calculation
fn bezier_point(p0: Point2, p1: Point2, p2: Point2, p3: Point2, t: f32) -> Point2 {
    let u = 1.0 - t;
    let tt = t * t;
    let uu = u * u;
    let uuu = uu * u;
    let ttt = tt * t;

    pt2(
        uuu * p0.x + 3.0 * uu * t * p1.x + 3.0 * u * tt * p2.x + ttt * p3.x,
        uuu * p0.y + 3.0 * uu * t * p1.y + 3.0 * u * tt * p2.y + ttt * p3.y,
    )
}
