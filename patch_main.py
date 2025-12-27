#!/usr/bin/env python3
import re

# Read the file
with open('apps/daemon/src/main.rs', 'r') as f:
    content = f.read()

# 1. Add imports
imports_pattern = r'(use text_tools::\{SaveFileSink, OutputFormat\};)'
imports_replacement = r'''\1

// Layout editor and visualizer modules
mod layout_editor;
mod patch_visualizer;
use layout_editor::LayoutEditor;'''
content = re.sub(imports_pattern, imports_replacement, content)

# 2. Add Patch to imports
content = content.replace(
    'use talisman_core::{Source, Sink, Signal, PatchBay, PluginManager, PluginModuleAdapter, ModuleRuntime};',
    'use talisman_core::{Source, Sink, Signal, PatchBay, PluginManager, PluginModuleAdapter, ModuleRuntime, Patch};'
)

# 3. Add layout_editor field to Model
model_pattern = r'(    // Audio State\n    audio_buffer: VecDeque<f32>, // Circular buffer for oscilloscope\n)'
model_replacement = r'''\1    
    // Layout Editor State (Phase 5)
    layout_editor: LayoutEditor,
'''
content = re.sub(model_pattern, model_replacement, content)

# 4. Initialize layout_editor in model() function
init_pattern = r'(        module_host,\n        plugin_manager,\n    \}\n\})'
init_replacement = r'''        module_host,
        plugin_manager,
        layout_editor: LayoutEditor::new(),
    }
}'''
content = re.sub(init_pattern, init_replacement, content)

# 5. Add else block to key_pressed function
keybind_pattern = r'(             \},\n            _ => \{\}\n        \}\n    \}\n\}\n\nfn raw_window_event)'
keybind_replacement = r'''             },
            _ => {}
        }
    } else {
        // Non-Ctrl keys
        match key {
            Key::E => {
                model.layout_editor.toggle_edit_mode();
                if model.layout_editor.edit_mode {
                    log::info!("Layout edit mode: ENABLED");
                } else {
                    log::info!("Layout edit mode: DISABLED");
                    model.layout.save();
                }
            },
            Key::Escape => {
                if model.layout_editor.edit_mode {
                    if model.layout_editor.dragging_tile.is_some() {
                        model.layout_editor.cancel_drag();
                    } else if model.layout_editor.resize_handle.is_some() {
                        model.layout_editor.cancel_resize();
                    } else {
                        model.layout_editor.toggle_edit_mode();
                        log::info!("Layout edit mode: DISABLED");
                        model.layout.save();
                    }
                }
            },
            _ => {}
        }
    }
}

fn raw_window_event'''
content = re.sub(keybind_pattern, keybind_replacement, content, flags=re.MULTILINE)

# Write the modified content
with open('apps/daemon/src/main.rs', 'w') as f:
    f.write(content)

print("Successfully patched main.rs")
