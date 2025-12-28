//! Text Input Tile - Simple egui TextEdit for testing

use nannou::prelude::*;
use std::sync::{Arc, Mutex};
use super::{TileRenderer, RenderContext};

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
        // Text input is updated via egui in render
    }
    
    fn render(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.08, 0.08, 0.08, 0.9));
        
        // Border
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .no_fill()
            .stroke_weight(1.0)
            .stroke(srgba(0.0, 1.0, 1.0, 0.5));
        
        // Draw current text (egui handles actual input)
        if let Ok(text) = self.text_buffer.lock() {
            let display = if text.is_empty() {
                "[Type here via egui...]".to_string()
            } else {
                text.clone()
            };
            
            draw.text(&display)
                .xy(rect.xy())
                .color(srgb(0.0, 1.0, 1.0))
                .font_size(16);
        }
        
        // Label
        draw.text("INPUT")
            .xy(pt2(rect.x(), rect.top() - 20.0))
            .color(srgba(0.5, 0.5, 0.5, 1.0))
            .font_size(12);
        
        // Note: Actual egui TextEdit is rendered in the main update loop
        // This just shows the current value
        let _ = ctx; // Unused for now, egui rendered separately
    }
    
    fn get_display_text(&self) -> Option<String> {
        self.text_buffer.lock().ok().map(|s| s.clone())
    }
}
