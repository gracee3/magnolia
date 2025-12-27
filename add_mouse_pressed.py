#!/usr/bin/env python3
"""
Add edit mode mouse handling to mouse_pressed function
"""
with open('apps/daemon/src/main.rs', 'r') as f:
    lines = f.readlines()

# Find the start of mouse_pressed function
start_idx = None
for i, line in enumerate(lines):
    if 'fn mouse_pressed(app: &App, model: &mut Model, button: MouseButton)' in line:
        start_idx = i
        break

if not start_idx:
    print("Could not find mouse_pressed function")
    exit(1)

# Find the line after "if model.egui.ctx().wants_pointer_input() { return; }"
insert_idx = None
for i in range(start_idx, start_idx + 20):
    if 'model.context_menu = None;' in lines[i]:
        insert_idx = i + 1
        break

if insert_idx:
    edit_mode_handling = '''
    // Phase 5: Edit mode mouse handling
    if model.layout_editor.edit_mode && button == MouseButton::Left {
        let mouse_pos = app.mouse.position();
        let col_sizes = model.layout.resolve_tracks(&model.layout.config.columns, app.window_rect().w());
        let row_sizes = model.layout.resolve_tracks(&model.layout.config.rows, app.window_rect().h());
        
        // Check if clicking on a resize handle (8px tolerance)
        let mut x_accum = app.window_rect().left();
        for (i, &width) in col_sizes.iter().enumerate() {
            x_accum += width;
            let handle_x = x_accum;
            if (mouse_pos.x - handle_x).abs() < 8.0 && i < col_sizes.len() - 1 {
                model.layout_editor.start_resize_column(i, mouse_pos.x);
                log::info!("Started resizing column {}", i);
                return;
            }
        }
        
        let mut y_accum = app.window_rect().top();
        for (i, &height) in row_sizes.iter().enumerate() {
            y_accum -= height;
            let handle_y = y_accum;
            if (mouse_pos.y - handle_y).abs() < 8.0 && i < row_sizes.len() - 1 {
                model.layout_editor.start_resize_row(i, mouse_pos.y);
                log::info!("Started resizing row {}", i);
                return;
            }
        }
        
        // Check if clicking on a tile to start dragging
        for tile in &model.layout.config.tiles {
            if let Some(rect) = model.layout.calculate_rect(tile) {
                if rect.contains(mouse_pos) {
                    let offset = pt2(mouse_pos.x - rect.x(), mouse_pos.y - rect.y());
                    model.layout_editor.start_drag(
                        tile.id.clone(),
                        tile.col,
                        tile.row,
                        offset
                    );
                    log::info!("Started dragging tile: {}", tile.id);
                    return;
                }
            }
        }
    }
    
'''
    lines.insert(insert_idx, edit_mode_handling)
    
    with open('apps/daemon/src/main.rs', 'w') as f:
        f.writelines(lines)
    print(f"Successfully added edit mode handling at line {insert_idx}")
else:
    print("Could not find insertion point")
