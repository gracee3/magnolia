//! Clock Tile - 24-hour digital clock display (HH:MM:SS)
//!
//! Monitor mode: Shows current time
//! Control mode: Settings for format (12/24hr), show seconds, etc.

use nannou::prelude::*;
use nannou_egui::egui;
use chrono::Local;
use serde::{Deserialize, Serialize};
use super::{TileRenderer, RenderContext, BindableAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeFormat {
    TwentyFourHour,
    TwelveHour,
}

impl Default for TimeFormat {
    fn default() -> Self {
        Self::TwentyFourHour
    }
}

pub struct ClockTile {
    current_time: String,
    format: TimeFormat,
    show_seconds: bool,
    show_date: bool,
}

impl ClockTile {
    pub fn new() -> Self {
        Self {
            current_time: String::new(),
            format: TimeFormat::TwentyFourHour,
            show_seconds: true,
            show_date: false,
        }
    }
    
    fn format_time(&self) -> String {
        let now = Local::now();
        let time_str = match (self.format, self.show_seconds) {
            (TimeFormat::TwentyFourHour, true) => now.format("%H:%M:%S").to_string(),
            (TimeFormat::TwentyFourHour, false) => now.format("%H:%M").to_string(),
            (TimeFormat::TwelveHour, true) => now.format("%I:%M:%S %p").to_string(),
            (TimeFormat::TwelveHour, false) => now.format("%I:%M %p").to_string(),
        };
        
        if self.show_date {
            format!("{}\n{}", now.format("%Y-%m-%d"), time_str)
        } else {
            time_str
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
        self.current_time = self.format_time();
    }
    
    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
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
    
    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.98));
        
        // Title
        draw.text("CLOCK SETTINGS")
            .xy(pt2(rect.x(), rect.top() - 30.0))
            .color(CYAN)
            .font_size(18);
        
        // Large time preview
        let preview_rect = Rect::from_x_y_w_h(
            rect.x(),
            rect.y() + 50.0,
            rect.w() * 0.8,
            rect.h() * 0.3,
        );
        
        let font_size = (preview_rect.h() * 0.6).min(120.0) as u32;
        draw.text(&self.current_time)
            .xy(preview_rect.xy())
            .color(srgb(0.0, 1.0, 1.0))
            .font_size(font_size);
        
        // Egui controls removed - migrated to Settings Modal
        
        false
    }
    
    fn settings_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "enum": ["TwentyFourHour", "TwelveHour"],
                    "default": "TwentyFourHour"
                },
                "show_seconds": {
                    "type": "boolean",
                    "default": true
                },
                "show_date": {
                    "type": "boolean",
                    "default": false
                }
            }
        }))
    }
    
    fn apply_settings(&mut self, settings: &serde_json::Value) {
        if let Some(fmt) = settings.get("format").and_then(|v| v.as_str()) {
            self.format = match fmt {
                "TwelveHour" => TimeFormat::TwelveHour,
                _ => TimeFormat::TwentyFourHour,
            };
        }
        if let Some(s) = settings.get("show_seconds").and_then(|v| v.as_bool()) {
            self.show_seconds = s;
        }
        if let Some(d) = settings.get("show_date").and_then(|v| v.as_bool()) {
            self.show_date = d;
        }
    }
    
    fn get_settings(&self) -> serde_json::Value {
        serde_json::json!({
            "format": format!("{:?}", self.format),
            "show_seconds": self.show_seconds,
            "show_date": self.show_date
        })
    }
    
    fn bindable_actions(&self) -> Vec<BindableAction> {
        vec![
            BindableAction::new("toggle_format", "Toggle 12/24 Hour", false),
            BindableAction::new("toggle_seconds", "Toggle Seconds", true),
        ]
    }
    
    fn execute_action(&mut self, action: &str) -> bool {
        match action {
            "toggle_format" => {
                self.format = match self.format {
                    TimeFormat::TwentyFourHour => TimeFormat::TwelveHour,
                    TimeFormat::TwelveHour => TimeFormat::TwentyFourHour,
                };
                true
            },
            "toggle_seconds" => {
                self.show_seconds = !self.show_seconds;
                true
            },
            _ => false,
        }
    }
    
    fn get_display_text(&self) -> Option<String> {
        Some(self.current_time.clone())
    }
}
