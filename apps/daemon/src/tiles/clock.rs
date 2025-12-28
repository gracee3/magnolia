//! Clock Tile - 24-hour digital clock display (HH:MM:SS)

use nannou::prelude::*;
use chrono::Local;
use super::{TileRenderer, RenderContext};

pub struct ClockTile {
    current_time: String,
}

impl ClockTile {
    pub fn new() -> Self {
        Self {
            current_time: String::new(),
        }
    }
}

impl Default for ClockTile {
    fn default() -> Self {
        Self::new()
    }
}

impl TileRenderer for ClockTile {
    fn id(&self) -> &str { "clock" }
    
    fn name(&self) -> &str { "Digital Clock" }
    
    fn update(&mut self) {
        self.current_time = Local::now().format("%H:%M:%S").to_string();
    }
    
    fn render(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.05, 0.05, 0.1, 0.9));
        
        // Time display
        let font_size = (rect.h() * 0.3).min(72.0) as u32;
        draw.text(&self.current_time)
            .xy(rect.xy())
            .color(srgb(0.0, 1.0, 1.0))
            .font_size(font_size);
        
        // Label
        draw.text("CLOCK")
            .xy(pt2(rect.x(), rect.top() - 20.0))
            .color(srgba(0.5, 0.5, 0.5, 1.0))
            .font_size(12);
    }
    
    fn get_display_text(&self) -> Option<String> {
        Some(self.current_time.clone())
    }
}
