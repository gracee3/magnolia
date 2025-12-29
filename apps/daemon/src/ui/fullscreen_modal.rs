//! Full-Screen Modal Rendering System
//!
//! Provides maximize-style presentation for all modals with consistent
//! animations, dark theme, and keyboard-first navigation.

#![allow(dead_code)] // Framework functions used progressively during migration

use nannou::prelude::*;

/// Animation speed for modal open/close (per frame)
pub const ANIM_SPEED: f32 = 0.12;

/// Modal margin from screen edges
pub const MODAL_MARGIN: f32 = 0.0;

/// Header height for modal title bar
pub const HEADER_HEIGHT: f32 = 50.0;

/// Content padding inside modal
pub const CONTENT_PADDING: f32 = 20.0;

/// Animation state for a modal
#[derive(Debug, Clone, Default)]
pub struct ModalAnim {
    /// Animation progress 0.0 (closed) to 1.0 (fully open)
    pub factor: f32,
    /// Whether closing animation is active
    pub closing: bool,
}

impl ModalAnim {
    pub fn new() -> Self {
        Self { factor: 0.0, closing: false }
    }

    /// Update animation state. Returns true if animation completed closing.
    pub fn update(&mut self) -> bool {
        if self.closing {
            self.factor = (self.factor - ANIM_SPEED).max(0.0);
            if self.factor <= 0.0 {
                return true; // Closing complete
            }
        } else {
            self.factor = (self.factor + ANIM_SPEED).min(1.0);
        }
        false
    }

    /// Start close animation
    pub fn start_close(&mut self) {
        self.closing = true;
    }

    /// Reset to initial state
    pub fn reset(&mut self) {
        self.factor = 0.0;
        self.closing = false;
    }

    /// Get eased animation factor (cubic ease in-out)
    pub fn eased(&self) -> f32 {
        let t = self.factor;
        if t < 0.5 {
            4.0 * t * t * t
        } else {
            (t - 1.0) * (2.0 * t - 2.0) * (2.0 * t - 2.0) + 1.0
        }
    }
}

/// Calculate the modal content rect based on animation state
pub fn calculate_modal_rect(window_rect: Rect, anim: &ModalAnim) -> Rect {
    let t = anim.eased();
    
    // Animate from center of screen (scale up)
    let target = window_rect.pad(MODAL_MARGIN);
    let center = window_rect.xy();
    
    // Scale from 0.8x size to 1.0x size during animation
    let scale = 0.8 + 0.2 * t;
    let w = target.w() * scale;
    let h = target.h() * scale;
    
    // Fade in alpha is handled separately
    Rect::from_x_y_w_h(center.x, center.y, w, h)
}

/// Draw modal background with dark theme and border
pub fn draw_modal_background(draw: &Draw, rect: Rect, anim: &ModalAnim) {
    let alpha = anim.eased();
    
    // Dark backdrop (covers entire screen)
    draw.rect()
        .xy(rect.xy())
        .wh(rect.wh() * 1.5) // Extend beyond modal to cover screen
        .color(rgba(0.0, 0.0, 0.0, 0.85 * alpha));
    
    // Modal background
    draw.rect()
        .xy(rect.xy())
        .wh(rect.wh())
        .color(rgba(0.04, 0.04, 0.05, alpha));
    
    // Reactive cyan border
    draw.rect()
        .xy(rect.xy())
        .wh(rect.wh())
        .no_fill()
        .stroke(rgba(0.0, 1.0, 1.0, 0.8 * alpha))
        .stroke_weight(2.0);
}

/// Draw modal header with title. Returns the content rect below header.
pub fn draw_modal_header(draw: &Draw, rect: Rect, title: &str, anim: &ModalAnim) -> Rect {
    let alpha = anim.eased();
    let header_rect = Rect::from_x_y_w_h(
        rect.x(),
        rect.top() - HEADER_HEIGHT / 2.0,
        rect.w(),
        HEADER_HEIGHT,
    );
    
    // Header background
    draw.rect()
        .xy(header_rect.xy())
        .wh(header_rect.wh())
        .color(rgba(0.02, 0.02, 0.03, alpha));
    
    // Separator line
    draw.line()
        .start(pt2(rect.left() + 10.0, rect.top() - HEADER_HEIGHT))
        .end(pt2(rect.right() - 10.0, rect.top() - HEADER_HEIGHT))
        .color(rgba(0.0, 1.0, 1.0, 0.3 * alpha))
        .stroke_weight(1.0);
    
    // Title text
    draw.text(title)
        .xy(pt2(rect.left() + CONTENT_PADDING + 100.0, rect.top() - HEADER_HEIGHT / 2.0))
        .color(rgba(0.0, 1.0, 1.0, alpha))
        .font_size(20)
        .left_justify();
    
    // ESC hint
    draw.text("[ESC] Close")
        .xy(pt2(rect.right() - 60.0, rect.top() - HEADER_HEIGHT / 2.0))
        .color(rgba(0.4, 0.4, 0.4, alpha))
        .font_size(12);
    
    // Return content rect (below header)
    Rect::from_corners(
        pt2(rect.left() + CONTENT_PADDING, rect.bottom() + CONTENT_PADDING),
        pt2(rect.right() - CONTENT_PADDING, rect.top() - HEADER_HEIGHT - CONTENT_PADDING),
    )
}

/// Draw a section header within the modal content
pub fn draw_section_header(draw: &Draw, y: f32, left: f32, text: &str, alpha: f32) {
    draw.text(text)
        .xy(pt2(left + 5.0, y))
        .color(rgba(0.4, 0.4, 0.5, alpha))
        .font_size(11)
        .left_justify();
}

/// Draw a text label
pub fn draw_label(draw: &Draw, x: f32, y: f32, text: &str, alpha: f32) {
    draw.text(text)
        .xy(pt2(x, y))
        .color(rgba(0.8, 0.8, 0.8, alpha))
        .font_size(14)
        .left_justify();
}

/// Draw a highlighted/selected label
pub fn draw_label_highlight(draw: &Draw, x: f32, y: f32, text: &str, alpha: f32) {
    draw.text(text)
        .xy(pt2(x, y))
        .color(rgba(0.0, 1.0, 1.0, alpha))
        .font_size(14)
        .left_justify();
}

/// Draw a muted/secondary label
pub fn draw_label_muted(draw: &Draw, x: f32, y: f32, text: &str, alpha: f32) {
    draw.text(text)
        .xy(pt2(x, y))
        .color(rgba(0.5, 0.5, 0.5, alpha))
        .font_size(12)
        .left_justify();
}

/// Draw a value display (e.g., for stats)
pub fn draw_value(draw: &Draw, x: f32, y: f32, label: &str, value: &str, alpha: f32) {
    draw.text(label)
        .xy(pt2(x, y))
        .color(rgba(0.6, 0.6, 0.6, alpha))
        .font_size(13)
        .left_justify();
    
    draw.text(value)
        .xy(pt2(x + 120.0, y))
        .color(rgba(0.0, 1.0, 1.0, alpha))
        .font_size(13)
        .left_justify();
}

/// Draw a button-like region (returns true if it would be "hovered" at given position)
pub fn draw_button(
    draw: &Draw,
    rect: Rect,
    text: &str,
    selected: bool,
    alpha: f32,
) {
    let bg_color = if selected {
        rgba(0.0, 0.4, 0.4, 0.5 * alpha)
    } else {
        rgba(0.1, 0.1, 0.12, alpha)
    };
    
    let border_color = if selected {
        rgba(0.0, 1.0, 1.0, 0.8 * alpha)
    } else {
        rgba(0.3, 0.3, 0.35, alpha)
    };
    
    let text_color = if selected {
        rgba(0.0, 1.0, 1.0, alpha)
    } else {
        rgba(0.7, 0.7, 0.7, alpha)
    };
    
    draw.rect()
        .xy(rect.xy())
        .wh(rect.wh())
        .color(bg_color);
    
    draw.rect()
        .xy(rect.xy())
        .wh(rect.wh())
        .no_fill()
        .stroke(border_color)
        .stroke_weight(1.5);
    
    draw.text(text)
        .xy(rect.xy())
        .color(text_color)
        .font_size(14);
}

/// Draw a danger/warning button
pub fn draw_button_danger(draw: &Draw, rect: Rect, text: &str, selected: bool, alpha: f32) {
    let bg_color = if selected {
        rgba(0.4, 0.1, 0.1, 0.5 * alpha)
    } else {
        rgba(0.15, 0.08, 0.08, alpha)
    };
    
    let border_color = if selected {
        rgba(1.0, 0.3, 0.3, 0.8 * alpha)
    } else {
        rgba(0.4, 0.2, 0.2, alpha)
    };
    
    let text_color = if selected {
        rgba(1.0, 0.4, 0.4, alpha)
    } else {
        rgba(0.8, 0.4, 0.4, alpha)
    };
    
    draw.rect()
        .xy(rect.xy())
        .wh(rect.wh())
        .color(bg_color);
    
    draw.rect()
        .xy(rect.xy())
        .wh(rect.wh())
        .no_fill()
        .stroke(border_color)
        .stroke_weight(1.5);
    
    draw.text(text)
        .xy(rect.xy())
        .color(text_color)
        .font_size(14);
}

/// Draw a list item (selectable)
pub fn draw_list_item(
    draw: &Draw,
    rect: Rect,
    text: &str,
    selected: bool,
    alpha: f32,
) {
    if selected {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(rgba(0.0, 0.3, 0.3, 0.4 * alpha));
        
        // Selection indicator
        draw.rect()
            .xy(pt2(rect.left() + 3.0, rect.y()))
            .wh(vec2(4.0, rect.h() * 0.6))
            .color(rgba(0.0, 1.0, 1.0, alpha));
    }
    
    let text_color = if selected {
        rgba(0.0, 1.0, 1.0, alpha)
    } else {
        rgba(0.7, 0.7, 0.7, alpha)
    };
    
    draw.text(text)
        .xy(pt2(rect.left() + 15.0, rect.y()))
        .color(text_color)
        .font_size(14)
        .left_justify();
}

/// Draw a horizontal separator line
pub fn draw_separator(draw: &Draw, y: f32, left: f32, right: f32, alpha: f32) {
    draw.line()
        .start(pt2(left, y))
        .end(pt2(right, y))
        .color(rgba(0.2, 0.2, 0.25, alpha))
        .stroke_weight(1.0);
}

/// Draw a status indicator (colored dot + text)
pub fn draw_status(draw: &Draw, x: f32, y: f32, status: &str, is_active: bool, alpha: f32) {
    let color = if is_active {
        rgba(0.2, 1.0, 0.4, alpha)
    } else {
        rgba(1.0, 0.3, 0.3, alpha)
    };
    
    // Status dot
    draw.ellipse()
        .xy(pt2(x, y))
        .radius(4.0)
        .color(color);
    
    // Status text
    draw.text(status)
        .xy(pt2(x + 15.0, y))
        .color(color)
        .font_size(13)
        .left_justify();
}
