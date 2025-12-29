//! Input Mode & Keyboard Navigation
//!
//! Centralized keyboard-first navigation system for Talisman.
//! Arrow keys work in all modes, ESC cascades through navigation hierarchy.

use talisman_core::{LayoutConfig, TileConfig};
use nannou::prelude::Key;
use crate::tiles::TileRegistry;

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

/// Actions requested by the input system to be handled by the main app loop
#[derive(Debug, Clone, PartialEq)]
pub enum AppAction {
    /// Connect/Disconnect patches, save layout, etc.
    SaveLayout,
    /// Quit the application (only mapped to Ctrl+Q)
    QuitApp,
    /// Copy text to clipboard
    Copy { text: String },
    /// Open the global settings modal
    OpenGlobalSettings,

    /// Open the add-tile picker modal at a specific grid cell
    OpenAddTilePicker { col: usize, row: usize },
    /// Open the patch bay modal
    OpenPatchBay,
    /// Open settings for a specific tile (effectively maximizing it)
    OpenTileSettings { tile_id: String },
    /// Toggle maximizing the currently selected tile
    ToggleMaximize,
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

    /// Main entry point for processing key presses.
    /// Returns an optional AppAction for side-effects.
    pub fn handle_key(
        &mut self,
        key: Key,
        ctrl_pressed: bool,
        layout: &mut LayoutConfig,
        registry: &TileRegistry,
    ) -> Option<AppAction> {
        
        // 1. Global Shortcuts (Ctrl+)
        if ctrl_pressed {
            match key {
                Key::Q => return Some(AppAction::QuitApp),
                Key::C => {
                    // Copy logic
                    if let Some(tile_id) = self.selected_tile_id() {
                        if let Some(text) = registry.get_display_text(tile_id) {
                            return Some(AppAction::Copy { text });
                        }
                    }
                    return None;
                },
                _ => return None,
            }
        }

        // 2. Tile-Specific Keybinds
        if self.has_selection() {
             if self.dispatch_tile_keybind(key, layout, registry) {
                 return None; 
             }
        }

        // 3. Navigation & Mode specific handling
        match key {
            // === ARROW KEYS - Always navigate ===
            Key::Up | Key::Down | Key::Left | Key::Right => {
                let direction = match key {
                    Key::Up => Direction::Up,
                    Key::Down => Direction::Down,
                    Key::Left => Direction::Left,
                    Key::Right => Direction::Right,
                    _ => unreachable!(),
                };
                
                match self.mode {
                    InputMode::Normal | InputMode::Patch => {
                        // Smart tile-to-tile navigation
                        if let Some(tile_id) = self.navigate_to_adjacent_tile(direction, layout) {
                            log::debug!("Navigated to tile: {}", tile_id);
                        } else {
                            // No tile found, deselect
                             self.deselect();
                             log::debug!("No adjacent tile in that direction");
                        }
                    },
                    InputMode::Layout => {
                         self.handle_layout_arrows(direction, layout);
                    },
                }
            },
            
            // === E - Settings / Edit / Layout Mode ===
            Key::E => {
                match self.mode {
                    InputMode::Normal => {
                         if let Some(tile_id) = self.selected_tile_id() {
                             return Some(AppAction::OpenTileSettings { tile_id: tile_id.to_string() });
                         } else {
                             self.enter_layout_mode();
                             log::info!("Entered layout mode");
                         }
                    },
                    InputMode::Layout => {
                        if self.has_selection() {
                            if self.enter_resize_mode(layout) {
                                log::info!("Entered resize mode");
                            }
                        }
                    },
                    InputMode::Patch => {
                        // Port selection deferred
                    }
                }
            },

            // === P - Patch Mode ===
            Key::P => {
                match self.mode {
                    InputMode::Normal => {
                        self.enter_patch_mode();
                        return Some(AppAction::OpenPatchBay); 
                    },
                    InputMode::Patch => {
                         self.exit_patch_mode();
                    },
                    InputMode::Layout => {
                        self.exit_layout_mode();
                        self.enter_patch_mode();
                    }
                }
            },

            // === SPACE - Move/Resize Toggle ===
            Key::Space => {
                if self.mode == InputMode::Layout {
                     match &self.layout_state {
                        LayoutSubState::Resize { .. } => {
                             self.enter_move_mode(layout);
                        },
                        LayoutSubState::Move { .. } => {
                             self.enter_resize_mode(layout);
                        },
                        _ => {}
                     }
                }
            },

            // === ENTER - Confirm / Select ===
            Key::Return => {
                match self.mode {
                    InputMode::Normal | InputMode::Patch => {
                        if !self.has_selection() {
                            if let Some(_tile_id) = self.select_tile_at_cursor(layout) {
                                // selection updated
                            }
                        } else {
                            return Some(AppAction::ToggleMaximize);
                        }
                    },
                    InputMode::Layout => {
                        match &self.layout_state {
                             LayoutSubState::Resize { .. } | LayoutSubState::Move { .. } => {
                                 self.exit_resize_move_mode();
                                 return Some(AppAction::SaveLayout);
                             },
                             LayoutSubState::Navigation => {
                                 if self.select_tile_at_cursor(layout).is_none() {
                                     // Empty cell: open add-tile picker at cursor
                                     let (col, row) = self.cursor;
                                     return Some(AppAction::OpenAddTilePicker { col, row });
                                 }
                             }
                        }
                    }
                }
            },

            // === ESCAPE - Back / Cancel ===
            Key::Escape => {
                let result = self.handle_escape();
                match result {
                    EscapeResult::ExitedSubMode => {
                        if let Some(bounds) = self.get_original_bounds() {
                             if let Some(tile_id) = self.selected_tile_id() {
                                 if let Some(tile) = layout.tiles.iter_mut().find(|t| t.id == tile_id) {
                                     tile.col = bounds.0;
                                     tile.row = bounds.1;
                                     tile.colspan = Some(bounds.2);
                                     tile.rowspan = Some(bounds.3);
                                 }
                             }
                        }
                    },
                     _ => {}
                }
            },

            // === G - Global Settings ===
            Key::G => {
                if self.mode == InputMode::Normal {
                    return Some(AppAction::OpenGlobalSettings);
                }
            },

            // === L - Layout Mode Toggle ===
            Key::L => {
                match self.mode {
                    InputMode::Normal => {
                        self.enter_layout_mode();
                        log::info!("Entered layout mode via L key");
                    },
                    InputMode::Layout => {
                        self.exit_layout_mode();
                        log::info!("Exited layout mode via L key");
                    },
                    InputMode::Patch => {
                        // L does nothing in patch mode
                    }
                }
            },

            // === D / Delete - Delete selected tile (Layout mode only) ===
            Key::D | Key::Delete | Key::Back => {
                if self.mode == InputMode::Layout {
                    if let Some(tile_id) = self.selected_tile_id().map(|s| s.to_string()) {
                        // Remove tile from layout
                        layout.tiles.retain(|t| t.id != tile_id);
                        self.deselect();
                        log::info!("Deleted tile: {}", tile_id);
                        return Some(AppAction::SaveLayout);
                    }
                }
            },

            // === A - Add tile (Layout mode only) ===
            Key::A => {
                if self.mode == InputMode::Layout {
                    // Open add-tile picker at cursor
                    let (col, row) = self.cursor;
                    return Some(AppAction::OpenAddTilePicker { col, row });
                }
            },

            // === Tab - Cycle through tiles ===
            Key::Tab => {
                self.cycle_tile_selection(layout, true);
            },

            _ => {}
        }

        None
    }

    /// Internal helper to dispatch tile-specific keybinds
    fn dispatch_tile_keybind(
        &mut self, 
        key: Key, 
        layout: &LayoutConfig, 
        registry: &TileRegistry
    ) -> bool {
        if let Some(tile_id) = self.selected_tile_id() {
            if let Some(tile_config) = layout.tiles.iter().find(|t| t.id == tile_id) {
                let keybinds = &tile_config.settings.keybinds;
                if !keybinds.is_empty() {
                    let key_str = format!("{:?}", key).to_lowercase();
                    for (action, bound_key) in keybinds {
                        if bound_key.to_lowercase() == key_str {
                            log::info!("Executing keybind: {} -> {} on tile {}", bound_key, action, tile_id);
                            return registry.execute_action(&tile_config.module, action);
                        }
                    }
                }
            }
        }
        false
    }

    fn handle_layout_arrows(&mut self, direction: Direction, layout: &mut LayoutConfig) {
        // Snapshot for safe rollback if conflict resolution is impossible.
        let before_tiles = layout.tiles.clone();

        // Clone state to avoid holding borrow on self
        let state = self.layout_state.clone(); 
        
        match state {
            LayoutSubState::Resize { tile_id, .. } => {
                let (delta_colspan, delta_rowspan) = Self::resize_direction(direction);
                // Find tile index to mutate
                if let Some(idx) = layout.tiles.iter().position(|t| t.id == tile_id) {
                     let tile = &mut layout.tiles[idx];
                     let current_colspan = tile.colspan.unwrap_or(1);
                     let current_rowspan = tile.rowspan.unwrap_or(1);

                     // Apply resize
                     let new_colspan = (current_colspan as i32 + delta_colspan).max(1) as usize;
                     let new_rowspan = (current_rowspan as i32 + delta_rowspan).max(1) as usize;
                     
                     tile.colspan = Some(new_colspan);
                     tile.rowspan = Some(new_rowspan);

					// Enforce no-overlap policy by resolving conflicts immediately.
					if let Err(e) = layout.resolve_conflicts(Some(&tile_id)) {
						log::warn!("Layout conflict resolution failed (reverting resize): {}", e);
						layout.tiles = before_tiles;
					}
                }
            },
            LayoutSubState::Move { tile_id, .. } => {
                self.navigate(direction);
                let (new_col, new_row) = self.cursor;

                if let Some(tile) = layout.tiles.iter_mut().find(|t| t.id == tile_id) {
                    tile.col = new_col;
                    tile.row = new_row;
                }

				// Enforce no-overlap policy by resolving conflicts immediately.
				if let Err(e) = layout.resolve_conflicts(Some(&tile_id)) {
					log::warn!("Layout conflict resolution failed (reverting move): {}", e);
					layout.tiles = before_tiles;
				}
            },
            LayoutSubState::Navigation => {
                self.navigate(direction);
                if let Some(_tile_id) = self.select_tile_at_cursor(layout) {
                     // select_tile_at_cursor updates selection state
                } else {
                    self.deselect();
                }
            }
        }
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

    /// Navigate to adjacent tile in the given direction using smart adjacency detection
    /// Returns the tile ID if navigation successful, None otherwise
    pub fn navigate_to_adjacent_tile(&mut self, direction: Direction, layout: &LayoutConfig) -> Option<String> {
        // Get current tile if any
        let current_tile = Self::get_tile_at_cell(layout, self.cursor.0, self.cursor.1);
        
        if let Some(current) = current_tile {
            // Find the best adjacent tile in the specified direction
            if let Some(adjacent) = self.find_adjacent_tile(current, direction, layout) {
                // Move cursor to the adjacent tile's position
                self.cursor = (adjacent.col, adjacent.row);
                self.selection = SelectionState::TileSelected { tile_id: adjacent.id.clone() };
                return Some(adjacent.id.clone());
            } else {
                // No adjacent tile found, try moving cursor anyway
                self.navigate(direction);
                if let Some(tile) = Self::get_tile_at_cell(layout, self.cursor.0, self.cursor.1) {
                    if tile.id != current.id {
                        self.selection = SelectionState::TileSelected { tile_id: tile.id.clone() };
                        return Some(tile.id.clone());
                    }
                }
            }
        } else {
            // No current tile, just move cursor and select whatever is there
            self.navigate(direction);
            if let Some(tile) = Self::get_tile_at_cell(layout, self.cursor.0, self.cursor.1) {
                self.selection = SelectionState::TileSelected { tile_id: tile.id.clone() };
                return Some(tile.id.clone());
            }
        }
        
        None
    }

    /// Find the adjacent tile in a direction that has maximum overlap
    fn find_adjacent_tile<'a>(
        &self,
        current: &TileConfig,
        direction: Direction,
        layout: &'a LayoutConfig,
    ) -> Option<&'a TileConfig> {
        let cur_col = current.col;
        let cur_row = current.row;
        let cur_colspan = current.colspan.unwrap_or(1);
        let cur_rowspan = current.rowspan.unwrap_or(1);

        let mut best_tile: Option<&TileConfig> = None;
        let mut best_overlap = 0;

        for tile in &layout.tiles {
            if tile.id == current.id {
                continue;
            }

            let tile_col = tile.col;
            let tile_row = tile.row;
            let tile_colspan = tile.colspan.unwrap_or(1);
            let tile_rowspan = tile.rowspan.unwrap_or(1);

            // Calculate overlap based on direction
            let (is_adjacent, overlap) = match direction {
                Direction::Up => {
                    // Tile must be directly above
                    if tile_row + tile_rowspan == cur_row {
                        // Calculate horizontal overlap
                        let overlap_start = cur_col.max(tile_col);
                        let overlap_end = (cur_col + cur_colspan).min(tile_col + tile_colspan);
                        if overlap_end > overlap_start {
                            (true, overlap_end - overlap_start)
                        } else {
                            (false, 0)
                        }
                    } else {
                        (false, 0)
                    }
                },
                Direction::Down => {
                    // Tile must be directly below
                    if cur_row + cur_rowspan == tile_row {
                        // Calculate horizontal overlap
                        let overlap_start = cur_col.max(tile_col);
                        let overlap_end = (cur_col + cur_colspan).min(tile_col + tile_colspan);
                        if overlap_end > overlap_start {
                            (true, overlap_end - overlap_start)
                        } else {
                            (false, 0)
                        }
                    } else {
                        (false, 0)
                    }
                },
                Direction::Left => {
                    // Tile must be directly to the left
                    if tile_col + tile_colspan == cur_col {
                        // Calculate vertical overlap
                        let overlap_start = cur_row.max(tile_row);
                        let overlap_end = (cur_row + cur_rowspan).min(tile_row + tile_rowspan);
                        if overlap_end > overlap_start {
                            (true, overlap_end - overlap_start)
                        } else {
                            (false, 0)
                        }
                    } else {
                        (false, 0)
                    }
                },
                Direction::Right => {
                    // Tile must be directly to the right
                    if cur_col + cur_colspan == tile_col {
                        // Calculate vertical overlap
                        let overlap_start = cur_row.max(tile_row);
                        let overlap_end = (cur_row + cur_rowspan).min(tile_row + tile_rowspan);
                        if overlap_end > overlap_start {
                            (true, overlap_end - overlap_start)
                        } else {
                            (false, 0)
                        }
                    } else {
                        (false, 0)
                    }
                },
            };

            if is_adjacent && overlap > best_overlap {
                best_overlap = overlap;
                best_tile = Some(tile);
            }
        }

        best_tile
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

    /// Cycle through tiles in row-major order (Tab navigation)
    pub fn cycle_tile_selection(&mut self, layout: &LayoutConfig, forward: bool) {
        if layout.tiles.is_empty() {
            return;
        }

        // Sort tiles by row then column for consistent ordering
        let mut sorted_tiles: Vec<&TileConfig> = layout.tiles.iter().collect();
        sorted_tiles.sort_by(|a, b| {
            let row_cmp = a.row.cmp(&b.row);
            if row_cmp == std::cmp::Ordering::Equal {
                a.col.cmp(&b.col)
            } else {
                row_cmp
            }
        });

        // Find current index
        let current_idx = if let Some(current_id) = self.selected_tile_id() {
            sorted_tiles.iter().position(|t| t.id == current_id)
        } else {
            None
        };

        // Calculate next index
        let next_idx = match current_idx {
            Some(idx) => {
                if forward {
                    (idx + 1) % sorted_tiles.len()
                } else {
                    if idx == 0 { sorted_tiles.len() - 1 } else { idx - 1 }
                }
            },
            None => 0, // Start at first tile if nothing selected
        };

        // Select the next tile
        let next_tile = sorted_tiles[next_idx];
        self.cursor = (next_tile.col, next_tile.row);
        self.selection = SelectionState::TileSelected { tile_id: next_tile.id.clone() };
        log::debug!("Tab navigation: selected tile {}", next_tile.id);
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
        // Extract data first to avoid borrow conflict
        let resize_data = match &self.layout_state {
            LayoutSubState::Resize { tile_id, original_bounds } => {
                Some((tile_id.clone(), *original_bounds))
            }
            _ => None,
        };

        if let Some((tile_id, bounds)) = resize_data {
            self.layout_state = LayoutSubState::Move {
                tile_id,
                original_bounds: bounds,
            };
            return true;
        }
            
        // If not in resize mode, check selection
        if let SelectionState::TileSelected { tile_id } = &self.selection {
            let tile_id = tile_id.clone();
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
                return true;
            }
        }
        
        false
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
                    // ESC at root with no selection = no-op
                    // Use Ctrl+Q to exit the application
                    EscapeResult::NoAction
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
    /// ESC at root with no selection - no action taken
    NoAction,
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
        
        // Manual internal selection
        nav.selection = SelectionState::TileSelected { tile_id: "test_tile".to_string() };
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
}


