//! Talisman Theme Constants
//!
//! Centralized visual styling for the dark/cyan reactive aesthetic.

#[cfg(feature = "tile-rendering")]
use nannou::prelude::*;

// =============================================================================
// COLORS
// =============================================================================

/// Primary reactive cyan color (selection, focus)
pub fn reactive_cyan() -> (u8, u8, u8) {
    (0, 255, 255)
}

/// Dark background color
pub fn dark_bg() -> (u8, u8, u8) {
    (10, 10, 15)
}

/// Muted stroke color for unselected elements
pub fn muted_stroke() -> (u8, u8, u8) {
    (60, 60, 70)
}

/// Warning/alert color
pub fn warning_red() -> (u8, u8, u8) {
    (255, 80, 80)
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
// HELPERS
// =============================================================================

/// Get selection border color with optional alpha
#[cfg(feature = "tile-rendering")]
pub fn selection_color(alpha: f32) -> LinSrgba {
    LinSrgba::new(0.0, 1.0, 1.0, alpha)
}

/// Get muted color with optional alpha  
#[cfg(feature = "tile-rendering")]
pub fn muted_color(alpha: f32) -> LinSrgba {
    LinSrgba::new(0.25, 0.25, 0.28, alpha)
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
#[cfg(feature = "tile-rendering")]
pub fn border_for_state(is_selected: bool, is_hovered: bool) -> LinSrgba {
    if is_selected {
        selection_color(0.9)
    } else if is_hovered {
        selection_color(0.5)
    } else {
        muted_color(0.6)
    }
}
