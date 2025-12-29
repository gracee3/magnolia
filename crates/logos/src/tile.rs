//! Text Input Tile - Egui TextEdit for text entry
//!
//! Monitor mode: Read-only preview of current text
//! Control mode: Full text editor (placeholder)

use nannou::prelude::*;
// use nannou_egui::egui removed
use std::sync::{Arc, Mutex};
use talisman_core::{TileRenderer, RenderContext, BindableAction};

pub struct TextInputTile {
    text_buffer: Arc<Mutex<String>>,
}

impl TextInputTile {
    pub fn new() -> Self {
        Self {
            text_buffer: Arc::new(Mutex::new(String::new())),
        }
    }
    
    /// Get a clone of the text buffer for external use
    pub fn get_buffer(&self) -> Arc<Mutex<String>> {
        self.text_buffer.clone()
    }
}

impl Default for TextInputTile {
    fn default() -> Self {
        Self::new()
    }
}

impl TileRenderer for TextInputTile {
    fn id(&self) -> &str { "text_input" }
    
    fn name(&self) -> &str { "Text Input" }
    
    fn update(&mut self) {
        // Text input is updated via Nannou controls (placeholder)
    }
    
    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.05, 0.05, 0.08, 0.9));
        
        // Get current text
        let text = self.text_buffer.lock()
            .map(|t| t.clone())
            .unwrap_or_default();
        
        // Preview (truncated, read-only)
        let preview = if text.is_empty() {
            "[No text - double-click to edit]".to_string()
        } else if text.len() > 80 {
            format!("{}...", &text[..80])
        } else {
            text.replace('\n', " ").replace('\r', "")
        };
        
        draw.text(&preview)
            .xy(rect.xy())
            .w(rect.w() - 20.0)
            .color(srgba(0.6, 0.6, 0.6, 1.0))
            .font_size(14);
        
        // Word count indicator
        let word_count = self.text_buffer.lock()
            .map(|t| t.split_whitespace().count())
            .unwrap_or(0);
        draw.text(&format!("{} words", word_count))
            .xy(pt2(rect.right() - 50.0, rect.bottom() + 15.0))
            .color(srgba(0.4, 0.4, 0.4, 0.8))
            .font_size(10);
        
        // Label
        draw.text("TEXT INPUT")
            .xy(pt2(rect.x(), rect.top() - 15.0))
            .color(srgba(0.5, 0.5, 0.5, 1.0))
            .font_size(11);
        
        // Edit mode hint
        draw.text("⏎ to edit")
            .xy(pt2(rect.left() + 40.0, rect.bottom() + 15.0))
            .color(srgba(0.0, 0.6, 0.6, 0.6))
            .font_size(10);
    }
    
    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.98));
        
        // Title
        draw.text("TEXT EDITOR")
            .xy(pt2(rect.x(), rect.top() - 30.0))
            .color(CYAN)
            .font_size(18);
        
        // Word count
        let word_count = self.text_buffer.lock()
            .map(|t| t.split_whitespace().count())
            .unwrap_or(0);
        let char_count = self.text_buffer.lock()
            .map(|t| t.len())
            .unwrap_or(0);
        
        draw.text(&format!("{} words | {} chars", word_count, char_count))
            .xy(pt2(rect.x(), rect.top() - 55.0))
            .color(srgba(0.5, 0.5, 0.5, 1.0))
            .font_size(12);
        
        false
    }
    
    fn bindable_actions(&self) -> Vec<BindableAction> {
        vec![
            BindableAction::new("clear", "Clear Text", false),
        ]
    }
    
    fn execute_action(&mut self, action: &str) -> bool {
        match action {
            "clear" => {
                if let Ok(mut text) = self.text_buffer.lock() {
                    text.clear();
                }
                true
            },
            _ => false,
        }
    }
    
    fn get_display_text(&self) -> Option<String> {
        self.text_buffer.lock().ok().map(|s| s.clone())
    }
}
