//! Kamea Tile - Generative sigil visualization
//!
//! Creates visual sigils from text input using SHA256 hash
//! and random walk on a grid (Digital Kamea method).

use nannou::prelude::*;
use sha2::{Sha256, Digest};
use kamea::{generate_path, SigilConfig};
use std::sync::{Arc, Mutex};
use super::{TileRenderer, RenderContext};

pub struct KameaTile {
    current_text: Arc<Mutex<String>>,
    path_points: Vec<Point2>,
    config: SigilConfig,
    last_text_hash: [u8; 32],
}

impl KameaTile {
    pub fn new() -> Self {
        Self {
            current_text: Arc::new(Mutex::new(String::new())),
            path_points: Vec::new(),
            config: SigilConfig {
                spacing: 40.0,
                stroke_weight: 2.0,
                grid_rows: 4,
                grid_cols: 4,
            },
            last_text_hash: [0u8; 32],
        }
    }
    
    /// Get the shared text buffer for external input
    pub fn get_text_buffer(&self) -> Arc<Mutex<String>> {
        self.current_text.clone()
    }
    
    /// Set the input text (triggers regeneration on next update)
    pub fn set_text(&self, text: &str) {
        if let Ok(mut t) = self.current_text.lock() {
            *t = text.to_string();
        }
    }
    
    fn regenerate_path(&mut self, text: &str) {
        // Hash the text
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let result = hasher.finalize();
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&result);
        
        // Check if we need to regenerate
        if seed == self.last_text_hash && !self.path_points.is_empty() {
            return;
        }
        self.last_text_hash = seed;
        
        // Adjust grid size based on text length
        let len_factor = text.len().min(100);
        let size = if len_factor > 10 { 5 } else { 4 };
        self.config.grid_rows = size;
        self.config.grid_cols = size;
        
        // Generate the path
        self.path_points = generate_path(seed, self.config)
            .into_iter()
            .map(|(x, y)| pt2(x, y))
            .collect();
    }
}

impl Default for KameaTile {
    fn default() -> Self {
        Self::new()
    }
}

impl TileRenderer for KameaTile {
    fn id(&self) -> &str { "kamea" }
    
    fn name(&self) -> &str { "Kamea Sigil" }
    
    fn update(&mut self) {
        let text = self.current_text.lock()
            .map(|t| t.clone())
            .unwrap_or_default();
        
        if !text.is_empty() {
            self.regenerate_path(&text);
        }
    }
    
    fn render(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.95));
        
        // Calculate scale to fit grid in rect
        let grid_size = self.config.grid_cols.max(self.config.grid_rows) as f32;
        let scale = (rect.w().min(rect.h()) * 0.8) / (grid_size * self.config.spacing);
        
        // Draw grid dots
        let dot_color = srgba(0.2, 0.3, 0.4, 0.5);
        for row in 0..self.config.grid_rows {
            for col in 0..self.config.grid_cols {
                let x = (col as f32 - (self.config.grid_cols as f32 - 1.0) / 2.0) * self.config.spacing * scale;
                let y = (row as f32 - (self.config.grid_rows as f32 - 1.0) / 2.0) * self.config.spacing * scale;
                draw.ellipse()
                    .xy(rect.xy() + vec2(x, y))
                    .radius(3.0)
                    .color(dot_color);
            }
        }
        
        // Draw sigil path
        if self.path_points.len() >= 2 {
            let path_color = srgb(0.0, 1.0, 1.0); // Cyan
            
            for window in self.path_points.windows(2) {
                let offset = vec2(rect.x(), rect.y());
                let p0 = window[0] * scale + offset;
                let p1 = window[1] * scale + offset;
                
                // Glow effect (wider, transparent)
                draw.line()
                    .start(p0)
                    .end(p1)
                    .weight(6.0)
                    .color(srgba(0.0, 1.0, 1.0, 0.2));
                
                // Main line
                draw.line()
                    .start(p0)
                    .end(p1)
                    .weight(self.config.stroke_weight)
                    .color(path_color);
            }
            
            // Start marker - Circle ○
            if let Some(start) = self.path_points.first() {
                let offset = vec2(rect.x(), rect.y());
                let pos = *start * scale + offset;
                draw.ellipse()
                    .xy(pos)
                    .radius(8.0)
                    .no_fill()
                    .stroke_weight(2.0)
                    .stroke(srgb(0.0, 1.0, 0.5));
            }
            
            // End marker - Cross ×
            if let Some(end) = self.path_points.last() {
                let offset = vec2(rect.x(), rect.y());
                let pos = *end * scale + offset;
                let size = 6.0;
                draw.line()
                    .start(pos + vec2(-size, -size))
                    .end(pos + vec2(size, size))
                    .weight(2.0)
                    .color(srgb(1.0, 0.3, 0.3));
                draw.line()
                    .start(pos + vec2(size, -size))
                    .end(pos + vec2(-size, size))
                    .weight(2.0)
                    .color(srgb(1.0, 0.3, 0.3));
            }
        }
        
        // Label
        draw.text("KAMEA")
            .xy(pt2(rect.x(), rect.top() - 20.0))
            .color(srgba(0.5, 0.5, 0.5, 1.0))
            .font_size(12);
    }
    
    fn get_display_text(&self) -> Option<String> {
        self.current_text.lock().ok().map(|t| t.clone())
    }
}
