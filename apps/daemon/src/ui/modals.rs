use std::vec::Vec;

/// Modal types for the unified modal stack
#[derive(Debug, Clone, PartialEq)]
pub enum ModalState {
    /// Patch Bay modal
    PatchBay,
    /// Global settings modal
    GlobalSettings,
    /// Layout manager modal
    LayoutManager,
    /// Tile maximized/control view (tile_id)
    Maximized { tile_id: String },
    /// Add tile picker (in layout mode)
    AddTilePicker { cursor_col: usize, cursor_row: usize, selected_idx: usize },
}

/// Modal stack for hierarchical modal management
/// ESC always pops the top modal, providing consistent navigation
#[derive(Debug, Default)]
pub struct ModalStack {
    stack: Vec<ModalState>,
}

impl ModalStack {
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Push a modal onto the stack
    pub fn push(&mut self, modal: ModalState) {
        // Don't push duplicate modals
        if self.stack.last() != Some(&modal) {
            self.stack.push(modal);
        }
    }

    /// Pop the top modal, returning it if present
    pub fn pop(&mut self) -> Option<ModalState> {
        self.stack.pop()
    }

    /// Check if any modal is open
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Check if patch bay is open
    pub fn is_patch_bay_open(&self) -> bool {
        self.stack.iter().any(|m| matches!(m, ModalState::PatchBay))
    }

    /// Check if global settings is open
    pub fn is_global_settings_open(&self) -> bool {
        self.stack.iter().any(|m| matches!(m, ModalState::GlobalSettings))
    }

    /// Check if layout manager is open
    pub fn is_layout_manager_open(&self) -> bool {
        self.stack.iter().any(|m| matches!(m, ModalState::LayoutManager))
    }

    /// Check if a tile is maximized
    pub fn get_maximized_tile(&self) -> Option<&str> {
        for modal in self.stack.iter().rev() {
            if let ModalState::Maximized { tile_id } = modal {
                return Some(tile_id);
            }
        }
        None
    }



    /// Check if add tile picker is open
    pub fn get_add_tile_picker(&self) -> Option<(usize, usize, usize)> {
        for modal in self.stack.iter().rev() {
            if let ModalState::AddTilePicker { cursor_col, cursor_row, selected_idx } = modal {
                return Some((*cursor_col, *cursor_row, *selected_idx));
            }
        }
        None
    }

    pub fn open_add_tile_picker(&mut self, col: usize, row: usize) {
        self.push(ModalState::AddTilePicker { cursor_col: col, cursor_row: row, selected_idx: 0 });
    }

    pub fn move_add_tile_picker_selection(&mut self, delta: i32, len: usize) {
        if len == 0 {
            return;
        }
        if let Some(top) = self.stack.last_mut() {
            if let ModalState::AddTilePicker { selected_idx, .. } = top {
                let cur = *selected_idx as i32;
                let next = (cur + delta).rem_euclid(len as i32) as usize;
                *selected_idx = next;
            }
        }
    }

    /// Close a specific modal type (removes first match from top)
    pub fn close(&mut self, modal: &ModalState) {
        if let Some(pos) = self.stack.iter().rposition(|m| std::mem::discriminant(m) == std::mem::discriminant(modal)) {
            self.stack.remove(pos);
        }
    }
}
