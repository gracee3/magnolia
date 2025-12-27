use nannou::prelude::*;
use talisman_core::{LayoutConfig, TileConfig, Patch};

/// Edit state for a tile being dragged
#[derive(Debug, Clone)]
pub struct DragState {
    pub tile_id: String,
    pub offset: Vec2,
    pub original_col: usize,
    pub original_row: usize,
}

/// Edit state for resizing columns or rows
#[derive(Debug, Clone)]
pub enum ResizeHandle {
    Column { index: usize, start_x: f32 },
    Row { index: usize, start_y: f32 },
}

/// Main layout editor state
#[derive(Debug)]
pub struct LayoutEditor {
    pub edit_mode: bool,
    pub dragging_tile: Option<DragState>,
    pub resize_handle: Option<ResizeHandle>,
    pub selected_tile: Option<String>,
    pub show_grid_overlay: bool,
    pub preview_tracks: Option<(Vec<f32>, Vec<f32>)>, // (col_sizes, row_sizes)
}

impl LayoutEditor {
    pub fn new() -> Self {
        Self {
            edit_mode: false,
            dragging_tile: None,
            resize_handle: None,
            selected_tile: None,
            show_grid_overlay: false,
            preview_tracks: None,
        }
    }

    /// Toggle edit mode on/off
    pub fn toggle_edit_mode(&mut self) {
        self.edit_mode = !self.edit_mode;
        if !self.edit_mode {
            // Clear any editing state when exiting
            self.dragging_tile = None;
            self.resize_handle = None;
            self.preview_tracks = None;
        }
    }

    /// Start dragging a tile
    pub fn start_drag(&mut self, tile_id: String, tile_col: usize, tile_row: usize, mouse_offset: Vec2) {
        self.dragging_tile = Some(DragState {
            tile_id,
            offset: mouse_offset,
            original_col: tile_col,
            original_row: tile_row,
        });
    }

    /// Stop dragging and update tile position
    pub fn end_drag(&mut self, new_col: usize, new_row: usize, layout: &mut LayoutConfig) -> bool {
        if let Some(drag) = self.dragging_tile.take() {
            // Find and update the tile
            if let Some(tile) = layout.tiles.iter_mut().find(|t| t.id == drag.tile_id) {
                tile.col = new_col;
                tile.row = new_row;
                return true; // Indicates layout changed
            }
        }
        false
    }

    /// Cancel drag without changes
    pub fn cancel_drag(&mut self) {
        self.dragging_tile = None;
    }

    /// Start resizing a column or row
    pub fn start_resize_column(&mut self, index: usize, start_x: f32) {
        self.resize_handle = Some(ResizeHandle::Column { index, start_x });
    }

    pub fn start_resize_row(&mut self, index: usize, start_y: f32) {
        self.resize_handle = Some(ResizeHandle::Row { index, start_y });
    }

    /// Update preview during resize
    pub fn update_resize_preview(&mut self, col_sizes: Vec<f32>, row_sizes: Vec<f32>) {
        self.preview_tracks = Some((col_sizes, row_sizes));
    }

    /// Finish resizing and update layout config
    pub fn end_resize(&mut self, layout: &mut LayoutConfig, new_tracks: Vec<String>, is_column: bool) -> bool {
        self.resize_handle = None;
        self.preview_tracks = None;
        
        if is_column {
            layout.columns = new_tracks;
        } else {
            layout.rows = new_tracks;
        }
        true // Indicates layout changed
    }

    /// Cancel resize without changes
    pub fn cancel_resize(&mut self) {
        self.resize_handle = None;
        self.preview_tracks = None;
    }

    /// Select a tile for editing
    pub fn select_tile(&mut self, tile_id: Option<String>) {
        self.selected_tile = tile_id;
    }

    /// Get grid cell at mouse position
    pub fn get_grid_cell(&self, mouse_pos: Vec2, win_rect: Rect, col_sizes: &[f32], row_sizes: &[f32]) -> Option<(usize, usize)> {
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
}

/// Render the edit mode overlay
pub fn render_edit_overlay(draw: &Draw, win_rect: Rect, col_sizes: &[f32], row_sizes: &[f32]) {
    // Draw grid lines
    let grid_color = rgba(1.0, 1.0, 1.0, 0.2);
    
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

/// Render resize handles between columns/rows
pub fn render_resize_handles(draw: &Draw, win_rect: Rect, col_sizes: &[f32], row_sizes: &[f32]) {
    let handle_color = rgba(0.4, 0.8, 1.0, 0.6);
    let handle_size = 8.0;
    
    // Column handles
    let mut x = win_rect.left();
    for (i, &width) in col_sizes.iter().enumerate() {
        x += width;
        if i < col_sizes.len() - 1 {
            draw.rect()
                .x_y(x, 0.0)
                .w_h(handle_size, win_rect.h())
                .color(handle_color);
        }
    }
    
    // Row handles
    let mut y = win_rect.top();
    for (i, &height) in row_sizes.iter().enumerate() {
        y -= height;
        if i < row_sizes.len() - 1 {
            draw.rect()
                .x_y(0.0, y)
                .w_h(win_rect.w(), handle_size)
                .color(handle_color);
        }
    }
}

/// Render a ghost/preview of a tile being dragged
pub fn render_tile_ghost(draw: &Draw, rect: Rect) {
    draw.rect()
        .xy(rect.xy())
        .wh(rect.wh())
        .color(rgba(1.0, 1.0, 1.0, 0.3))
        .stroke(rgba(1.0, 1.0, 1.0, 0.8))
        .stroke_weight(2.0);
}
