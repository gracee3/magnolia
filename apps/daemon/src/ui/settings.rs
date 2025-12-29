use nannou::prelude::*;
use crate::ui::controls::{Form, UiInput, UiNav};
use crate::ui::modals::GlobalSettingsState;
use crate::ui::fullscreen_modal::{ModalAnim, draw_modal_header, draw_modal_background, calculate_modal_rect};

pub fn render(
    draw: &Draw,
    rect: Rect,
    state: &GlobalSettingsState,
    anim: &ModalAnim,
) {
    // Calculate animated modal rect
    let modal_rect = calculate_modal_rect(rect, anim);

    // 0. Draw Modal Background
    draw_modal_background(draw, modal_rect, anim);

    // 1. Draw Modal Container (Header)
    let content_rect = draw_modal_header(draw, modal_rect, "GLOBAL SETTINGS", anim);
    
    // 2. Draw Settings Form
    // Center the form
    let form_width = 500.0;
    let form_rect = Rect::from_x_y_w_h(content_rect.x(), content_rect.y(), form_width, content_rect.h());
    
    // Initialize Form
    let mut form = Form::begin(&state.focus, form_rect);
    
    // Audio Settings
    draw.text("AUDIO ENGINE")
        .xy(pt2(form_rect.left() + 10.0, form_rect.top() - 20.0))
        .color(CYAN)
        .font_size(14)
        .left_justify();
        
    form.stepper_row(
        draw, 
        "Sample Rate", 
        &state.sample_rate.to_string(), 
    );

    form.stepper_row(
        draw, 
        "Buffer Size", 
        &state.audio_buffer_size.to_string(), 
    );
    
    // UI Settings
    form.slider_row(
        draw, 
        "Theme Hue", 
        state.theme_hue, 
        0.0, 1.0, 
    );
    
    form.toggle_row(
        draw, 
        "Show Debug Stats", 
        state.show_debug_stats, 
    );
}

pub fn handle_key(
    key: Key,
    state: &mut GlobalSettingsState,
) -> bool {
    let input = UiInput::from_key(key, false, false); // Ctrl/shift not passed yet, todo
    
    // Escape Handling
    if let Some(UiNav::Escape) = input.nav {
        return false; // Let parent close
    }

    // 1. Handle Navigation
    // Manual navigation since Form::nav was removed or isn't usable without state
    if let Some(nav) = &input.nav {
        match nav {
            UiNav::Up => state.focus.focused = state.focus.focused.saturating_sub(1),
            UiNav::Down => state.focus.focused = (state.focus.focused + 1).min(3), // 4 rows (0-3)
            _ => {}
        }
    }
    
    // 2. Handle Value Changes
    // Indexes:
    // 0: Sample Rate
    // 1: Buffer Size
    // 2: Theme Hue
    // 3: Debug Stats
    
    if let Some(nav) = &input.nav {
        match state.focus.focused {
            0 => { // Sample Rate
                match nav {
                    UiNav::Left => {
                        if state.sample_rate > 44100 { state.sample_rate = 44100; }
                    },
                    UiNav::Right => {
                        if state.sample_rate < 48000 { state.sample_rate = 48000; }
                        else if state.sample_rate < 96000 { state.sample_rate = 96000; }
                    },
                    _ => {}
                }
            },
            1 => { // Buffer Size
                match nav {
                    UiNav::Left => {
                        if state.audio_buffer_size > 256 { state.audio_buffer_size /= 2; }
                    },
                    UiNav::Right => {
                         if state.audio_buffer_size < 2048 { state.audio_buffer_size *= 2; }
                    },
                    _ => {}
                }
            },
            2 => { // Theme Hue
                 match nav {
                     UiNav::Left => state.theme_hue = (state.theme_hue - 0.05).max(0.0),
                     UiNav::Right => state.theme_hue = (state.theme_hue + 0.05).min(1.0),
                     _ => {}
                 }
            },
            3 => { // Show Debug Stats
                 if let Some(UiNav::Enter) | Some(UiNav::Left) | Some(UiNav::Right) = input.nav {
                     state.show_debug_stats = !state.show_debug_stats;
                 }
            },
            _ => {}
        }
    }
    
    true // Consumed
}
