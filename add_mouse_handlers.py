#!/usr/bin/env python3
"""
Add mouse handlers to app construction and create handler functions
"""
import re

with open('apps/daemon/src/main.rs', 'r') as f:
    content = f.read()

# 1. Add mouse_released and mouse_moved to app construction
app_construction_pattern = r'(\.mouse_pressed\(mouse_pressed\))'
app_construction_replacement = r'''\1
        .mouse_released(mouse_released)
        .mouse_moved(mouse_moved)'''

content = re.sub(app_construction_pattern, app_construction_replacement, content)

# 2. Add mouse_released function before key_pressed
mouse_released_fn = '''
fn mouse_released(app: &App, model: &mut Model, button: MouseButton) {
    if button == MouseButton::Left && model.layout_editor.edit_mode {
        let mouse_pos = app.mouse.position();
        let col_sizes = model.layout.resolve_tracks(&model.layout.config.columns, app.window_rect().w());
        let row_sizes = model.layout.resolve_tracks(&model.layout.config.rows, app.window_rect().h());
        
        // End resize if resizing
        if let Some(handle) = &model.layout_editor.resize_handle {
            match handle {
                layout_editor::ResizeHandle::Column { index, .. } => {
                    // Calculate new column sizes
                    // For now, just cancel - full implementation needs track recalculation
                    model.layout_editor.cancel_resize();
                    log::info!("Column resize released (calculation TODO)");
                },
                layout_editor::ResizeHandle::Row { index, .. } => {
                    model.layout_editor.cancel_resize();
                    log::info!("Row resize released (calculation TODO)");
                }
            }
            model.layout.save();
            return;
        }
        
        // End tile drag if dragging
        if model.layout_editor.dragging_tile.is_some() {
            // Get grid cell under mouse
            if let Some((col, row)) = model.layout_editor.get_grid_cell(
                mouse_pos,
                app.window_rect(),
                &col_sizes,
                &row_sizes
            ) {
                if model.layout_editor.end_drag(col, row, &mut model.layout.config) {
                    log::info!("Moved tile to col={}, row={}", col, row);
                    model.layout.save();
                }
            } else {
                model.layout_editor.cancel_drag();
            }
        }
    }
}

fn mouse_moved(app: &App, model: &mut Model, pos: Point2) {
    if !model.layout_editor.edit_mode {
        return;
    }
    
    // Update resize preview if resizing
    if model.layout_editor.resize_handle.is_some() {
        // Preview resize would update here
        // For now, just acknowledge movement
    }
    
    // Update drag preview if dragging
    if model.layout_editor.dragging_tile.is_some() {
        // Drag preview updates automatically via render logic
    }
}

'''

# Find key_pressed function and insert before it
key_pressed_pattern = r'(fn key_pressed\(_app: &App, model: &mut Model, key: Key\) \{)'
content = re.sub(key_pressed_pattern, mouse_released_fn + r'\1', content)

with open('apps/daemon/src/main.rs', 'w') as f:
    f.write(content)

print("Successfully added mouse handlers")
