//! Input Mode & Keyboard Navigation
//!
//! Centralized keyboard-first navigation system for Talisman.
//! Arrow keys work in all modes, ESC cascades through navigation hierarchy.

use talisman_core::{LayoutConfig, TileConfig};

/// Top-level input mode
#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    /// Normal viewing mode - arrows navigate grid, select tiles
    Normal,
    /// Layout editing mode
    Layout,
    /// Patch bay mode - selecting tiles for patching
    Patch,
}

impl Default for InputMode {
    fn default() -> Self {
        InputMode::Normal
    }
}

/// Selection state within the current mode
#[derive(Debug, Clone, PartialEq)]
pub enum SelectionState {
    /// No tile selected, cursor navigating
    None,
    /// A tile is selected/focused
    TileSelected { tile_id: String },
}

impl Default for SelectionState {
    fn default() -> Self {
        SelectionState::None
    }
}

/// Layout mode sub-states
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutSubState {
    /// Navigating grid with cursor
    Navigation,
    /// Resizing the selected tile with arrow keys
    Resize {
        tile_id: String,
        original_bounds: (usize, usize, usize, usize), // col, row, colspan, rowspan
    },
    /// Moving the selected tile (shown as 1×1)
    Move {
        tile_id: String,
        original_bounds: (usize, usize, usize, usize),
    },
}

impl Default for LayoutSubState {
    fn default() -> Self {
        LayoutSubState::Navigation
    }
}

/// Direction for keyboard navigation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Central keyboard navigation state
#[derive(Debug)]
pub struct KeyboardNav {
    /// Current top-level mode
    pub mode: InputMode,
    /// Current selection state
    pub selection: SelectionState,
    /// Layout mode sub-state (only relevant when mode == Layout)
    pub layout_state: LayoutSubState,
    /// Grid cursor position (col, row)
    pub cursor: (usize, usize),
    /// Last selected tile (for ESC → re-select behavior)
    pub last_selected: Option<String>,
    /// Grid dimensions cache
    grid_cols: usize,
    grid_rows: usize,
}

impl Default for KeyboardNav {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardNav {
    pub fn new() -> Self {
        Self {
            mode: InputMode::Normal,
            selection: SelectionState::None,
            layout_state: LayoutSubState::Navigation,
            cursor: (0, 0),
            last_selected: None,
            grid_cols: 4,
            grid_rows: 4,
        }
    }

    /// Update cached grid dimensions
    pub fn set_grid_size(&mut self, cols: usize, rows: usize) {
        self.grid_cols = cols;
        self.grid_rows = rows;
        // Clamp cursor to valid range
        self.cursor.0 = self.cursor.0.min(cols.saturating_sub(1));
        self.cursor.1 = self.cursor.1.min(rows.saturating_sub(1));
    }

    /// Navigate cursor in direction, clamped to grid bounds
    pub fn navigate(&mut self, direction: Direction) {
        let (col, row) = self.cursor;
        self.cursor = match direction {
            Direction::Up => (col, row.saturating_sub(1)),
            Direction::Down => (col, (row + 1).min(self.grid_rows.saturating_sub(1))),
            Direction::Left => (col.saturating_sub(1), row),
            Direction::Right => ((col + 1).min(self.grid_cols.saturating_sub(1)), row),
        };
    }

    /// Select tile at current cursor position
    pub fn select_tile_at_cursor(&mut self, layout: &LayoutConfig) -> Option<String> {
        if let Some(tile) = Self::get_tile_at_cell(layout, self.cursor.0, self.cursor.1) {
            let tile_id = tile.id.clone();
            self.selection = SelectionState::TileSelected { tile_id: tile_id.clone() };
            Some(tile_id)
        } else {
            None
        }
    }

    /// Select a specific tile by ID
    pub fn select_tile(&mut self, tile_id: String) {
        self.selection = SelectionState::TileSelected { tile_id };
    }

    /// Deselect current tile, remembering it for potential re-select
    pub fn deselect(&mut self) {
        if let SelectionState::TileSelected { tile_id } = &self.selection {
            self.last_selected = Some(tile_id.clone());
        }
        self.selection = SelectionState::None;
    }

    /// Get the currently selected tile ID
    pub fn selected_tile_id(&self) -> Option<&str> {
        match &self.selection {
            SelectionState::TileSelected { tile_id } => Some(tile_id),
            SelectionState::None => None,
        }
    }

    /// Check if a tile is selected
    pub fn has_selection(&self) -> bool {
        matches!(self.selection, SelectionState::TileSelected { .. })
    }

    /// Enter layout mode (cursor position persists)
    pub fn enter_layout_mode(&mut self) {
        self.mode = InputMode::Layout;
        self.layout_state = LayoutSubState::Navigation;
    }

    /// Exit layout mode back to normal
    pub fn exit_layout_mode(&mut self) {
        self.mode = InputMode::Normal;
        self.layout_state = LayoutSubState::Navigation;
    }

    /// Enter patch mode
    pub fn enter_patch_mode(&mut self) {
        self.mode = InputMode::Patch;
    }

    /// Exit patch mode back to normal  
    pub fn exit_patch_mode(&mut self) {
        self.mode = InputMode::Normal;
    }

    /// Enter resize mode for the selected tile
    pub fn enter_resize_mode(&mut self, layout: &LayoutConfig) -> bool {
        if let SelectionState::TileSelected { tile_id } = &self.selection {
            if let Some(tile) = layout.tiles.iter().find(|t| t.id == *tile_id) {
                self.layout_state = LayoutSubState::Resize {
                    tile_id: tile_id.clone(),
                    original_bounds: (
                        tile.col,
                        tile.row,
                        tile.colspan.unwrap_or(1),
                        tile.rowspan.unwrap_or(1),
                    ),
                };
                return true;
            }
        }
        false
    }

    /// Enter move mode (shrink tile to 1×1, arrows move it)
    pub fn enter_move_mode(&mut self, layout: &LayoutConfig) -> bool {
        // Can enter move mode from resize mode or tile selected
        let tile_id = match &self.layout_state {
            LayoutSubState::Resize { tile_id, original_bounds } => {
                let tid = tile_id.clone();
                let bounds = *original_bounds;
                self.layout_state = LayoutSubState::Move {
                    tile_id: tid.clone(),
                    original_bounds: bounds,
                };
                return true;
            }
            _ => {
                if let SelectionState::TileSelected { tile_id } = &self.selection {
                    tile_id.clone()
                } else {
                    return false;
                }
            }
        };

        if let Some(tile) = layout.tiles.iter().find(|t| t.id == tile_id) {
            self.layout_state = LayoutSubState::Move {
                tile_id: tile_id.clone(),
                original_bounds: (
                    tile.col,
                    tile.row,
                    tile.colspan.unwrap_or(1),
                    tile.rowspan.unwrap_or(1),
                ),
            };
            true
        } else {
            false
        }
    }

    /// Exit resize/move mode back to tile selected
    pub fn exit_resize_move_mode(&mut self) {
        let tile_id = match &self.layout_state {
            LayoutSubState::Resize { tile_id, .. } => Some(tile_id.clone()),
            LayoutSubState::Move { tile_id, .. } => Some(tile_id.clone()),
            _ => None,
        };
        
        self.layout_state = LayoutSubState::Navigation;
        
        if let Some(id) = tile_id {
            self.selection = SelectionState::TileSelected { tile_id: id };
        }
    }

    /// Handle resize with arrow keys - returns (delta_colspan, delta_rowspan)
    pub fn resize_direction(direction: Direction) -> (i32, i32) {
        match direction {
            Direction::Up => (0, -1),
            Direction::Down => (0, 1),
            Direction::Left => (-1, 0),
            Direction::Right => (1, 0),
        }
    }

    /// Get original bounds if in resize/move mode
    pub fn get_original_bounds(&self) -> Option<(usize, usize, usize, usize)> {
        match &self.layout_state {
            LayoutSubState::Resize { original_bounds, .. } => Some(*original_bounds),
            LayoutSubState::Move { original_bounds, .. } => Some(*original_bounds),
            _ => None,
        }
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

    /// Handle ESC key - returns true if event was consumed
    pub fn handle_escape(&mut self) -> EscapeResult {
        match self.mode {
            InputMode::Normal => {
                if self.has_selection() {
                    self.deselect();
                    EscapeResult::Deselected
                } else {
                    // Second ESC with no selection - defer to caller for exit dialog
                    EscapeResult::ExitRequested
                }
            }
            InputMode::Layout => {
                match &self.layout_state {
                    LayoutSubState::Resize { .. } | LayoutSubState::Move { .. } => {
                        self.exit_resize_move_mode();
                        EscapeResult::ExitedSubMode
                    }
                    LayoutSubState::Navigation => {
                        if self.has_selection() {
                            self.deselect();
                            EscapeResult::Deselected
                        } else {
                            self.exit_layout_mode();
                            EscapeResult::ExitedMode
                        }
                    }
                }
            }
            InputMode::Patch => {
                if self.has_selection() {
                    self.deselect();
                    EscapeResult::Deselected
                } else {
                    self.exit_patch_mode();
                    EscapeResult::ExitedMode
                }
            }
        }
    }
}

/// Result of ESC key handling
#[derive(Debug, Clone, PartialEq)]
pub enum EscapeResult {
    /// Deselected the current tile
    Deselected,
    /// Exited a sub-mode (resize/move)
    ExitedSubMode,
    /// Exited the current mode (layout/patch) back to normal
    ExitedMode,
    /// No selection, exit dialog requested (deferred)
    ExitRequested,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_navigation() {
        let mut nav = KeyboardNav::new();
        nav.set_grid_size(4, 4);
        
        assert_eq!(nav.cursor, (0, 0));
        
        nav.navigate(Direction::Right);
        assert_eq!(nav.cursor, (1, 0));
        
        nav.navigate(Direction::Down);
        assert_eq!(nav.cursor, (1, 1));
        
        // Test clamping at edges
        nav.cursor = (3, 3);
        nav.navigate(Direction::Right);
        assert_eq!(nav.cursor, (3, 3));
        
        nav.navigate(Direction::Down);
        assert_eq!(nav.cursor, (3, 3));
    }

    #[test]
    fn test_selection() {
        let mut nav = KeyboardNav::new();
        
        assert!(!nav.has_selection());
        assert!(nav.selected_tile_id().is_none());
        
        nav.select_tile("test_tile".to_string());
        assert!(nav.has_selection());
        assert_eq!(nav.selected_tile_id(), Some("test_tile"));
        
        nav.deselect();
        assert!(!nav.has_selection());
        assert_eq!(nav.last_selected, Some("test_tile".to_string()));
    }

    #[test]
    fn test_mode_transitions() {
        let mut nav = KeyboardNav::new();
        
        assert_eq!(nav.mode, InputMode::Normal);
        
        nav.enter_layout_mode();
        assert_eq!(nav.mode, InputMode::Layout);
        
        nav.exit_layout_mode();
        assert_eq!(nav.mode, InputMode::Normal);
        
        nav.enter_patch_mode();
        assert_eq!(nav.mode, InputMode::Patch);
    }

    #[test]
    fn test_escape_cascade() {
        let mut nav = KeyboardNav::new();
        nav.select_tile("tile1".to_string());
        
        // ESC deselects tile
        let result = nav.handle_escape();
        assert_eq!(result, EscapeResult::Deselected);
        assert!(!nav.has_selection());
        
        // ESC with no selection requests exit
        let result = nav.handle_escape();
        assert_eq!(result, EscapeResult::ExitRequested);
    }
}
