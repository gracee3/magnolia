use nannou::prelude::*;
use crate::ui::controls::{List, UiInput, UiNav};
use crate::ui::modals::{PatchBayModalState, PatchBayPane};
use crate::ui::fullscreen_modal::{ModalAnim, draw_modal_header};
use talisman_core::{PatchBay, PortDirection};

pub fn render(
    draw: &Draw,
    rect: Rect,
    state: &PatchBayModalState, // Immutable state
    anim: &ModalAnim,
    patch_bay: &PatchBay,
) {
    // 1. Draw Modal Container
    let content_rect = draw_modal_header(draw, rect, "PATCH BAY", anim);
    // alpha unused

    // 2. Layout Columns (Modules | Ports | Connections)
    let col_w = content_rect.w() / 3.0;
    let modules_rect = Rect::from_x_y_w_h(content_rect.left() + col_w/2.0, content_rect.y(), col_w - 10.0, content_rect.h());
    let ports_rect = Rect::from_x_y_w_h(content_rect.x(), content_rect.y(), col_w - 10.0, content_rect.h());
    let patches_rect = Rect::from_x_y_w_h(content_rect.right() - col_w/2.0, content_rect.y(), col_w - 10.0, content_rect.h());

    // 3. Render Modules List
    // Filter active modules
    let modules = patch_bay.get_modules();
    
    // Highlight focused pane
    let modules_focused = state.focus_pane == PatchBayPane::Modules;
    let ports_focused = state.focus_pane == PatchBayPane::Ports;
    let patches_focused = state.focus_pane == PatchBayPane::Patches;
    
    // -- Modules Pane --
    // List::new takes immutable reference now
    let module_list = List::new(&state.modules_focus, modules_rect, modules.len(), 30.0)
        .with_title(if modules_focused { "> MODULES <" } else { "MODULES" });
        
    module_list.render(draw, |i, selected, rect| {
        let module = &modules[i];
        let color = if selected { CYAN } else { GREY };
        let name = &module.name;
        
        // Removed unused row_draw assignment
        if selected {
             draw.rect().xy(rect.xy()).wh(rect.wh()).color(rgba(0.0, 0.2, 0.2, 0.2))
                 .stroke(CYAN).stroke_weight(1.0);
        } else {
             draw.rect().xy(rect.xy()).wh(rect.wh()).color(rgba(0.0, 0.0, 0.0, 0.0));
        }
        
        let is_active_target = state.selected_module == i;
        if is_active_target {
            // Marker
             draw.rect()
                .x_y(rect.left() + 2.0, rect.y())
                .w_h(4.0, rect.h() - 4.0)
                .color(CYAN);
        }

        draw.text(name)
            .xy(rect.xy())
            .color(color)
            .font_size(14);
    });
    
    // -- Ports Pane --
    let selected_mod_idx = state.selected_module;
    let mut ports = Vec::new();
    if let Some(module) = modules.get(selected_mod_idx) {
        ports = module.ports.clone();
    }
    
    let port_list = List::new(&state.ports_focus, ports_rect, ports.len(), 30.0)
        .with_title(if ports_focused { "> PORTS <" } else { "PORTS" });
        
    port_list.render(draw, |i, selected, rect| {
        let port = &ports[i];
        let is_input = port.direction == PortDirection::Input;
        let dir_str = if is_input { "IN" } else { "OUT" };
        let color = if selected { CYAN } else { GREY };
        
        if selected {
             draw.rect().xy(rect.xy()).wh(rect.wh()).color(rgba(0.0, 0.2, 0.2, 0.2));
        }

        if let Some(module) = modules.get(selected_mod_idx) {
             if let Some((src_mod_id, src_port_id)) = &state.staged_source {
                 if &module.id == src_mod_id && &port.id == src_port_id {
                     draw.rect().xy(rect.xy()).wh(rect.wh()).no_fill().stroke(MAGENTA).stroke_weight(2.0);
                     draw.text("SOURCE")
                        .x_y(rect.right() - 30.0, rect.y())
                        .color(MAGENTA)
                        .font_size(10);
                 }
             }
        }
        
        draw.text(dir_str)
            .x_y(rect.left() + 20.0, rect.y())
            .color(if is_input { GREEN } else { ORANGE })
            .font_size(10);
            
        draw.text(&port.label)
             .x_y(rect.x(), rect.y())
             .color(color)
             .font_size(14);
             
        draw.text(&format!("{:?}", port.data_type))
            .x_y(rect.right() - 40.0, rect.y())
            .color(rgba(0.5, 0.5, 0.5, 0.8))
            .font_size(10);
    });

    // -- Patches Pane --
    let patches = patch_bay.get_patches(); 
    let patch_list = List::new(&state.patches_focus, patches_rect, patches.len(), 30.0)
         .with_title(if patches_focused { "> CONNECTIONS <" } else { "CONNECTIONS" });
         
    patch_list.render(draw, |i, selected, rect| {
         let patch = &patches[i];
         let color = if selected { CYAN } else { GREY };
         if selected {
              draw.rect().xy(rect.xy()).wh(rect.wh()).color(rgba(0.0, 0.2, 0.2, 0.2));
         }
         
         let label = format!("{}:{} -> {}:{}", 
             patch.source_module, patch.source_port,
             patch.sink_module, patch.sink_port);
             
         draw.text(&label)
             .xy(rect.xy())
             .color(color)
             .font_size(12);
    });
    
    // 4. Helper Text
    let hint = match state.focus_pane {
        PatchBayPane::Modules => "Select Module [Space/Enter] to Browse Ports",
        PatchBayPane::Ports => if state.staged_source.is_some() {
            "Select Sink Port [Enter] to Connect, [Esc] Cancel"
        } else {
            "Select Source Port [Enter] to Stage Connection"
        },
        PatchBayPane::Patches => "[Enter] to Disconnect, [Del] Delete",
    };
    
    draw.text(hint)
        .xy(pt2(rect.x(), rect.bottom() + 30.0))
        .color(CYAN)
        .font_size(14);
}

pub fn handle_key(
    key: Key, 
    state: &mut PatchBayModalState, 
    patch_bay: &mut PatchBay
) {
    let input = UiInput::from_key(key, false, false); 
    
    // Global Navigation
    if let Some(UiNav::Tab) = input.nav {
        state.focus_pane = match state.focus_pane {
            PatchBayPane::Modules => PatchBayPane::Ports,
            PatchBayPane::Ports => PatchBayPane::Patches,
            PatchBayPane::Patches => PatchBayPane::Modules,
        };
        return;
    }
    
    match state.focus_pane {
        PatchBayPane::Modules => {
            let module_count = patch_bay.get_modules().len();
            // Use static List::handle_nav
            if let Some(idx) = List::handle_nav(&mut state.modules_focus, module_count, &input) {
                state.selected_module = idx;
                state.focus_pane = PatchBayPane::Ports;
            } else {
                state.selected_module = state.modules_focus.focused;
            }
        }
        PatchBayPane::Ports => {
             let mut action_connect = None;
             let mut action_stage = None;
             
             {
                 let modules = patch_bay.get_modules();
                 if let Some(module) = modules.get(state.selected_module) {
                     let ports = &module.ports;
                     if let Some(idx) = List::handle_nav(&mut state.ports_focus, ports.len(), &input) {
                         if let Some(port) = ports.get(idx) {
                             if let Some((src_mod, src_port)) = &state.staged_source {
                                 // CONNECT intent
                                 if port.direction == PortDirection::Input {
                                     action_connect = Some((src_mod.clone(), src_port.clone(), module.id.clone(), port.id.clone()));
                                 }
                             } else {
                                 // STAGE intent
                                 if port.direction == PortDirection::Output {
                                     action_stage = Some((module.id.clone(), port.id.clone()));
                                 }
                             }
                         }
                     }
                 }
             }
             
             if let Some((src_m, src_p, dst_m, dst_p)) = action_connect {
                 let _ = patch_bay.connect(&src_m, &src_p, &dst_m, &dst_p);
                 state.staged_source = None;
                 state.focus_pane = PatchBayPane::Patches;
             }
             if let Some((m, p)) = action_stage {
                 state.staged_source = Some((m, p));
                 state.focus_pane = PatchBayPane::Modules;
             }
        }
        PatchBayPane::Patches => {
             let mut disconnect_id = None;
             {
                 let patches = patch_bay.get_patches();
                 if let Some(idx) = List::handle_nav(&mut state.patches_focus, patches.len(), &input) {
                     if let Some(patch) = patches.get(idx) {
                         disconnect_id = Some(patch.id.clone());
                     }
                 }
             }
             
             if let Some(id) = disconnect_id {
                 let _ = patch_bay.disconnect(&id);
             }
        }
    }
}
