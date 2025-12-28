//! Talisman Theme Constants
//!
//! Centralized visual styling for the dark/cyan reactive aesthetic.

use nannou::prelude::*;
use nannou_egui::egui;

// =============================================================================
// NANNOU COLORS (using functions instead of const due to PhantomData)
// =============================================================================

/// Primary reactive cyan color (selection, focus)
pub fn reactive_cyan() -> Srgb<u8> {
    Srgb::new(0, 255, 255)
}

/// Dark background color
pub fn dark_bg() -> Srgb<u8> {
    Srgb::new(10, 10, 15)
}

/// Muted stroke color for unselected elements
pub fn muted_stroke() -> Srgb<u8> {
    Srgb::new(60, 60, 70)
}

/// Warning/alert color
pub fn warning_red() -> Srgb<u8> {
    Srgb::new(255, 80, 80)
}

// =============================================================================
// STROKE WEIGHTS
// =============================================================================

/// Thick stroke for selected/focused elements
pub const SELECTION_STROKE: f32 = 4.0;

/// Medium stroke for highlighted elements  
pub const HIGHLIGHT_STROKE: f32 = 2.5;

/// Normal stroke for standard elements
pub const NORMAL_STROKE: f32 = 1.0;

// =============================================================================
// EGUI COLORS
// =============================================================================

/// Reactive cyan for egui
pub const EGUI_CYAN: egui::Color32 = egui::Color32::from_rgb(0, 255, 255);

/// Dark background for egui panels
pub const EGUI_DARK_BG: egui::Color32 = egui::Color32::from_rgb(20, 20, 25);

/// Muted text color
pub const EGUI_MUTED: egui::Color32 = egui::Color32::from_rgb(120, 120, 130);

// Runtime-constructed colors (not const)
pub fn egui_window_bg() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(15, 15, 20, 250)
}

pub fn egui_selected_bg() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(0, 255, 255, 30)
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Get selection border color with optional alpha
pub fn selection_color(alpha: f32) -> LinSrgba {
    LinSrgba::new(0.0, 1.0, 1.0, alpha)
}

/// Get muted color with optional alpha  
pub fn muted_color(alpha: f32) -> LinSrgba {
    LinSrgba::new(0.25, 0.25, 0.28, alpha)
}

/// Apply talisman dark/cyan theme to egui context
pub fn apply_egui_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    
    // Window styling
    style.visuals.window_fill = egui_window_bg();
    style.visuals.panel_fill = EGUI_DARK_BG;
    
    // Selection colors
    style.visuals.selection.bg_fill = egui_selected_bg();
    style.visuals.selection.stroke = egui::Stroke::new(2.0, EGUI_CYAN);
    
    // Widget styling
    style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(30, 30, 35);
    style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, EGUI_MUTED);
    
    style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(40, 40, 45);
    style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(180, 180, 190));
    
    style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(50, 50, 60);
    style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.5, EGUI_CYAN);
    
    style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(60, 60, 70);
    style.visuals.widgets.active.fg_stroke = egui::Stroke::new(2.0, EGUI_CYAN);
    
    ctx.set_style(style);
}

/// Get the appropriate stroke weight for an element based on selection state
pub fn stroke_for_state(is_selected: bool, is_hovered: bool) -> f32 {
    if is_selected {
        SELECTION_STROKE
    } else if is_hovered {
        HIGHLIGHT_STROKE
    } else {
        NORMAL_STROKE
    }
}

/// Get the appropriate border color for an element based on selection state
pub fn border_for_state(is_selected: bool, is_hovered: bool) -> LinSrgba {
    if is_selected {
        selection_color(0.9)
    } else if is_hovered {
        selection_color(0.5)
    } else {
        muted_color(0.6)
    }
}
