//! Nannou-native UI controls (keyboard-first)
//!
//! These are purely drawing primitives + simple layout helpers.
//! Input handling is expected to be handled by tiles via `TileRenderer::handle_key`
//! (or by the daemon for global modals).
#![allow(dead_code)] // Used progressively across tiles/modals; keep available without warning spam.

use nannou::prelude::*;

/// Spacing constants tuned for fullscreen "control mode" UIs.
pub const ROW_H: f32 = 34.0;
pub const ROW_GAP: f32 = 10.0;
pub const LABEL_X_PAD: f32 = 18.0;
pub const VALUE_X_PAD: f32 = 18.0;

#[derive(Debug, Clone, Copy)]
pub struct UiStyle {
    pub alpha: f32,
}

impl Default for UiStyle {
    fn default() -> Self {
        Self { alpha: 1.0 }
    }
}

pub fn draw_heading(draw: &Draw, pos: Point2, text: &str, style: UiStyle) {
    draw.text(text)
        .xy(pos)
        .color(rgba(0.0, 1.0, 1.0, style.alpha))
        .font_size(18);
}

pub fn draw_subtitle(draw: &Draw, pos: Point2, text: &str, style: UiStyle) {
    draw.text(text)
        .xy(pos)
        .color(rgba(0.55, 0.55, 0.6, style.alpha))
        .font_size(12);
}

pub fn draw_section(draw: &Draw, pos: Point2, text: &str, style: UiStyle) {
    draw.text(text)
        .xy(pos)
        .color(rgba(0.4, 0.5, 0.55, style.alpha))
        .font_size(11)
        .left_justify();
}

fn row_bg(draw: &Draw, rect: Rect, focused: bool, style: UiStyle) {
    let bg = if focused {
        rgba(0.0, 0.25, 0.25, 0.45 * style.alpha)
    } else {
        rgba(0.08, 0.08, 0.1, 0.9 * style.alpha)
    };
    draw.rect().xy(rect.xy()).wh(rect.wh()).color(bg);

    // Focus indicator bar
    if focused {
        draw.rect()
            .xy(pt2(rect.left() + 3.0, rect.y()))
            .wh(vec2(4.0, rect.h() * 0.65))
            .color(rgba(0.0, 1.0, 1.0, style.alpha));
    }
}

pub fn draw_toggle_row(draw: &Draw, rect: Rect, label: &str, value: bool, focused: bool, style: UiStyle) {
    row_bg(draw, rect, focused, style);

    draw.text(label)
        .xy(pt2(rect.left() + LABEL_X_PAD, rect.y()))
        .color(rgba(0.85, 0.85, 0.88, style.alpha))
        .font_size(14)
        .left_justify();

    let pill = if value { "ON" } else { "OFF" };
    let pill_color = if value {
        rgba(0.0, 1.0, 1.0, style.alpha)
    } else {
        rgba(0.5, 0.5, 0.55, style.alpha)
    };

    draw.text(pill)
        .xy(pt2(rect.right() - VALUE_X_PAD, rect.y()))
        .color(pill_color)
        .font_size(14)
        .right_justify();
}

pub fn draw_stepper_row(
    draw: &Draw,
    rect: Rect,
    label: &str,
    value_text: &str,
    focused: bool,
    style: UiStyle,
) {
    row_bg(draw, rect, focused, style);

    draw.text(label)
        .xy(pt2(rect.left() + LABEL_X_PAD, rect.y()))
        .color(rgba(0.85, 0.85, 0.88, style.alpha))
        .font_size(14)
        .left_justify();

    draw.text(&format!("< {} >", value_text))
        .xy(pt2(rect.right() - VALUE_X_PAD, rect.y()))
        .color(rgba(0.0, 1.0, 1.0, style.alpha))
        .font_size(14)
        .right_justify();
}

/// Helper: compute a vertical stack of row rects inside a container.
pub fn row_stack(container: Rect, count: usize) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(count);
    let total_h = (count as f32) * ROW_H + ((count.saturating_sub(1)) as f32) * ROW_GAP;
    let top = container.top() - ROW_H / 2.0;
    let start_y = top - (container.h() - total_h).max(0.0) * 0.5;

    for i in 0..count {
        let y = start_y - (i as f32) * (ROW_H + ROW_GAP);
        rects.push(Rect::from_x_y_w_h(container.x(), y, container.w(), ROW_H));
    }
    rects
}


