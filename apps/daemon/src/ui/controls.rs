//! Nannou-native UI controls (keyboard-first)
//!
//! These are purely drawing primitives + simple layout helpers.
//! Input handling is expected to be handled by tiles via `TileRenderer::handle_key`
//! (or by the daemon for global modals).
#![allow(dead_code)] // Used progressively across tiles/modals; keep available without warning spam.

use nannou::prelude::*;
use talisman_ui::{FontId, draw_text, TextAlignment};

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
    draw_text(
        draw,
        FontId::PlexSansBold,
        text,
        pos,
        18.0,
        rgba(0.0, 1.0, 1.0, style.alpha).into(),
        TextAlignment::Center,
    );
}

pub fn draw_subtitle(draw: &Draw, pos: Point2, text: &str, style: UiStyle) {
    draw_text(
        draw,
        FontId::PlexSansRegular,
        text,
        pos,
        12.0,
        rgba(0.55, 0.55, 0.6, style.alpha).into(),
        TextAlignment::Center,
    );
}

pub fn draw_section(draw: &Draw, pos: Point2, text: &str, style: UiStyle) {
    draw_text(
        draw,
        FontId::PlexSansBold,
        text,
        pos,
        11.0,
        rgba(0.4, 0.5, 0.55, style.alpha).into(),
        TextAlignment::Left,
    );
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

    draw_text(
        draw,
        FontId::PlexSansRegular,
        label,
        pt2(rect.left() + LABEL_X_PAD, rect.y()),
        14.0,
        rgba(0.85, 0.85, 0.88, style.alpha).into(),
        TextAlignment::Left,
    );

    let pill = if value { "ON" } else { "OFF" };
    let pill_color = if value {
        rgba(0.0, 1.0, 1.0, style.alpha)
    } else {
        rgba(0.5, 0.5, 0.55, style.alpha)
    };

    draw_text(
        draw,
        FontId::PlexSansBold,
        pill,
        pt2(rect.right() - VALUE_X_PAD, rect.y()),
        14.0,
        pill_color.into(),
        TextAlignment::Right,
    );
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

    draw_text(
        draw,
        FontId::PlexSansRegular,
        label,
        pt2(rect.left() + LABEL_X_PAD, rect.y()),
        14.0,
        rgba(0.85, 0.85, 0.88, style.alpha).into(),
        TextAlignment::Left,
    );

    draw_text(
        draw,
        FontId::PlexSansBold,
        &format!("< {} >", value_text),
        pt2(rect.right() - VALUE_X_PAD, rect.y()),
        14.0,
        rgba(0.0, 1.0, 1.0, style.alpha).into(),
        TextAlignment::Right,
    );
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

// === SHARED INTERACTION MODELS ===

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusModel {
    pub focused: usize,      // which row/item is focused
    pub editing: bool,       // whether the current control is in edit mode
    pub scroll: f32,         // optional for long forms/lists
}

impl Default for FocusModel {
    fn default() -> Self {
        Self {
            focused: 0,
            editing: false,
            scroll: 0.0,
        }
    }
}

impl FocusModel {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.focused = 0;
        } else if self.focused >= len {
            self.focused = len - 1;
        }
    }
}

/// Normalized navigation input (abstracts over keys)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiNav {
    Up, Down, Left, Right,
    Enter, Escape,
    PageUp, PageDown,
    Tab, BackTab,
}

#[derive(Debug, Clone)]
pub struct UiInput {
    pub nav: Option<UiNav>,
    pub shift: bool,
    pub ctrl: bool,
}

impl UiInput {
    pub fn from_key(key: Key, ctrl: bool, shift: bool) -> Self {
        let nav = match key {
            Key::Up => Some(UiNav::Up),
            Key::Down => Some(UiNav::Down),
            Key::Left => Some(UiNav::Left),
            Key::Right => Some(UiNav::Right),
            Key::Return | Key::Space => Some(UiNav::Enter),
            Key::Escape => Some(UiNav::Escape),
            Key::PageUp => Some(UiNav::PageUp),
            Key::PageDown => Some(UiNav::PageDown),
            Key::Tab => if shift { Some(UiNav::BackTab) } else { Some(UiNav::Tab) },
            _ => None,
        };
        Self { nav, shift, ctrl }
    }
}

/// Standard response from an interactive control
pub struct UiResponse {
    pub changed: bool,
    pub activated: bool,
}

// === FORM BUILDER ===

pub struct Form<'a> {
    pub focus: &'a FocusModel,
    pub row_count: usize,
    pub rect: Rect,
}

impl<'a> Form<'a> {
    pub fn begin(focus: &'a FocusModel, rect: Rect) -> Self {
        Self {
            focus,
            row_count: 0,
            rect,
        }
    }
    
    // nav() removed - handle focus mutation in handle_key
    
    fn next_row_rect(&mut self) -> (usize, Rect) {
        let i = self.row_count;
        // Simple linear stack from top
        let y = self.rect.top() - ROW_H / 2.0 - (i as f32) * (ROW_H + ROW_GAP);
        let r = Rect::from_x_y_w_h(self.rect.x(), y, self.rect.w(), ROW_H);
        self.row_count += 1;
        (i, r)
    }
    
    // --- Interactive Widgets (Render Only) ---

    pub fn toggle_row(
        &mut self,
        draw: &Draw,
        label: &str,
        value: bool,
    ) {
        let (idx, rect) = self.next_row_rect();
        let focused = self.focus.focused == idx;
        
        // Render
        draw_toggle_row(draw, rect, label, value, focused, UiStyle::default());
    }

    pub fn stepper_row(
        &mut self,
        draw: &Draw,
        label: &str,
        value_text: &str,
    ) {
        let (idx, rect) = self.next_row_rect();
        let focused = self.focus.focused == idx;

        // Render
        draw_stepper_row(draw, rect, label, value_text, focused, UiStyle::default());
    }
    
    pub fn slider_row(
        &mut self,
        draw: &Draw,
        label: &str,
        value: f32,
        min: f32,
        max: f32,
    ) {
        let (idx, rect) = self.next_row_rect();
        let focused = self.focus.focused == idx;
        
        // Render manually since we don't have draw_slider_row yet
        row_bg(draw, rect, focused, UiStyle::default());
        
        draw_text(
            draw,
            FontId::PlexSansRegular,
            label,
            pt2(rect.left() + LABEL_X_PAD, rect.y()),
            14.0,
            rgba(0.85, 0.85, 0.88, 1.0).into(),
            TextAlignment::Left,
        );
            
        // Bar
        let bar_w = rect.w() * 0.4;
        let bar_h = 4.0;
        let bar_x = rect.right() - VALUE_X_PAD - bar_w / 2.0;
        let norm = (value - min) / (max - min);
        
        // Track
        draw.rect()
            .x_y(bar_x, rect.y())
            .w_h(bar_w, bar_h)
            .color(rgba(0.3, 0.3, 0.35, 1.0));
            
        // Fill
        let fill_w = bar_w * norm;
        draw.rect()
            .x_y(bar_x - bar_w/2.0 + fill_w/2.0, rect.y())
            .w_h(fill_w, bar_h)
            .color(rgba(0.0, 1.0, 1.0, 1.0));
            
        // Knob
        if focused {
             draw.ellipse()
                .x_y(bar_x - bar_w/2.0 + fill_w, rect.y())
                .radius(4.0)
                .color(WHITE);
        }
    }
}

// === LIST BUILDER ===

pub struct List<'a> {
    pub focus: &'a FocusModel,
    pub rect: Rect,
    pub len: usize,
    pub item_h: f32,
    pub title: Option<&'a str>,
}

impl<'a> List<'a> {
    pub fn new(focus: &'a FocusModel, rect: Rect, len: usize, item_h: f32) -> Self {
        Self { focus, rect, len, item_h, title: None }
    }
    
    pub fn with_title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }
    
    /// Static helper for navigation logic (modifies focus)
    pub fn handle_nav(focus: &mut FocusModel, len: usize, input: &UiInput) -> Option<usize> {
        focus.clamp(len);
        
        // Return Some(index) if Activated (Enter)
        if let Some(nav) = &input.nav {
            match nav {
                UiNav::Up => {
                    focus.focused = focus.focused.saturating_sub(1);
                }
                UiNav::Down => {
                   focus.focused = (focus.focused + 1).min(len.saturating_sub(1));
                }
                UiNav::Enter => {
                    return Some(focus.focused);
                }
                _ => {}
            }
        }
        None
    }
    
    pub fn render<F>(&self, draw: &Draw, mut render_item: F) 
    where F: FnMut(usize, bool, Rect) 
    {
        // Draw title if present
        let mut list_rect = self.rect;
        if let Some(title) = self.title {
             draw_heading(draw, pt2(list_rect.x(), list_rect.top() - 15.0), title, UiStyle::default());
             list_rect = Rect::from_corners(
                pt2(list_rect.left(), list_rect.bottom()),
                pt2(list_rect.right(), list_rect.top() - 30.0),
             );
        }
        
        // Background
        draw.rect()
            .xy(list_rect.xy())
            .wh(list_rect.wh())
            .color(rgba(0.03, 0.03, 0.04, 0.8))
            .stroke(rgba(0.2, 0.2, 0.25, 1.0))
            .stroke_weight(1.0);
            
        if self.len == 0 {
            draw_text(
                draw,
                FontId::PlexSansRegular,
                "No items",
                list_rect.xy(),
                12.0,
                GREY.into(),
                TextAlignment::Center,
            );
            return;
        }

        // Clip to list rect for scrolling?
        // Nannou doesn't do scissoring easily in immediate mode without custom render pipeline work or manually culling.
        // We will manually cull.
        
        let visible_count = (list_rect.h() / self.item_h).floor() as usize;
        let start_idx = if self.focus.focused >= visible_count {
             self.focus.focused.saturating_sub(visible_count / 2) // Center focus if possible
        } else {
             0
        };
        // Clamp start_idx to ensure we show as many items as possible
        let start_idx = start_idx.min(self.len.saturating_sub(visible_count).max(0));
        
        let end_idx = (start_idx + visible_count).min(self.len);
        
        for i in start_idx..end_idx {
            // Layout ref: relative to list top
            let offset_y = (i - start_idx) as f32 * self.item_h;
            let y = list_rect.top() - self.item_h / 2.0 - offset_y;
            let item_rect = Rect::from_x_y_w_h(list_rect.x(), y, list_rect.w() - 4.0, self.item_h - 2.0);
            
            render_item(i, i == self.focus.focused, item_rect);
        }
        
        // Scrollbar if needed
        if self.len > visible_count {
             let scroll_w = 4.0;
             let scroll_h = list_rect.h() * (visible_count as f32 / self.len as f32);
             let scroll_track_h = list_rect.h();
             let scroll_y_pct = start_idx as f32 / (self.len - visible_count) as f32;
             let scroll_y = list_rect.top() - scroll_h/2.0 - (scroll_track_h - scroll_h) * scroll_y_pct;
             
             draw.rect()
                .x_y(list_rect.right() - 4.0, scroll_y)
                .w_h(scroll_w, scroll_h)
                .color(rgba(0.5, 0.5, 0.5, 0.5));
        }
    }
}



