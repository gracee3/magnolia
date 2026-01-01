//! Clock Tile - 24-hour digital clock display (HH:MM:SS)
//!
//! Monitor mode: Shows current time
//! Control mode: Settings for format (12/24hr), show seconds, etc.

use super::{BindableAction, RenderContext, TileRenderer};
use crate::ui::controls;
use chrono::Local;
use nannou::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use magnolia_ui::{draw_text, FontId, TextAlignment};

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

    // Control-mode UI state (keyboard focus)
    focused_control: Mutex<usize>,
}

impl ClockTile {
    pub fn new() -> Self {
        Self {
            current_time: String::new(),
            format: TimeFormat::TwentyFourHour,
            show_seconds: true,
            show_date: false,
            focused_control: Mutex::new(0),
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
    fn id(&self) -> &str {
        "clock"
    }

    fn name(&self) -> &str {
        "Digital Clock"
    }

    fn update(&mut self) {
        self.current_time = self.format_time();
    }

    fn render_monitor(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.05, 0.05, 0.1, 0.9));

        // Time display
        // Calculate font size based on both width and height to prevent overflow
        // "18:03:16" is 8 characters. Mono fonts are ~0.6w per h.
        // We need 8 * 0.6 * font_size < rect.w() * 0.9 (for margin)
        // font_size < rect.w() / 5.33 ... let's use / 6.5 to be safe with tracking.
        let font_size = (rect.h() * 0.25).min(rect.w() / 6.5).min(80.0);

        // Subtle pulse animation
        let pulse = (ctx.time.elapsed().as_secs_f32() * 1.5).sin() * 0.05 + 0.95;
        let color = srgba(0.0, 1.0, 1.0, pulse);

        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &self.current_time,
            rect.xy(),
            font_size,
            color,
            TextAlignment::Center,
        );

        // Label
        draw_text(
            draw,
            FontId::PlexSansRegular,
            "CLOCK",
            pt2(rect.x(), rect.top() - 20.0),
            12.0,
            srgba(0.5, 0.5, 0.5, 1.0),
            TextAlignment::Center,
        );
    }

    fn render_controls(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) -> bool {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.98));

        // Title
        controls::draw_heading(
            draw,
            pt2(rect.x(), rect.top() - 30.0),
            "CLOCK SETTINGS",
            controls::UiStyle { alpha: 1.0 },
        );
        controls::draw_subtitle(
            draw,
            pt2(rect.x(), rect.top() - 52.0),
            "↑/↓ focus   ←/→ change   Enter toggle",
            controls::UiStyle { alpha: 1.0 },
        );

        // Large time preview
        let preview_rect =
            Rect::from_x_y_w_h(rect.x(), rect.y() + 50.0, rect.w() * 0.8, rect.h() * 0.3);

        let font_size = (preview_rect.h() * 0.6).min(120.0);
        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &self.current_time,
            preview_rect.xy(),
            font_size,
            srgb(0.0, 1.0, 1.0).into(),
            TextAlignment::Center,
        );

        // Controls list (keyboard-only)
        let list_rect = Rect::from_x_y_w_h(
            rect.x(),
            rect.y() - rect.h() * 0.15,
            rect.w() * 0.70,
            rect.h() * 0.35,
        );
        let focused = self.focused_control.lock().map(|v| *v).unwrap_or(0);
        let rows = controls::row_stack(list_rect, 3);

        // 0: format stepper
        let fmt_label = match self.format {
            TimeFormat::TwentyFourHour => "24 Hour",
            TimeFormat::TwelveHour => "12 Hour",
        };
        controls::draw_stepper_row(
            draw,
            rows[0],
            "Format",
            fmt_label,
            focused == 0,
            controls::UiStyle { alpha: 1.0 },
        );

        // 1: seconds toggle
        controls::draw_toggle_row(
            draw,
            rows[1],
            "Show seconds",
            self.show_seconds,
            focused == 1,
            controls::UiStyle { alpha: 1.0 },
        );

        // 2: date toggle
        controls::draw_toggle_row(
            draw,
            rows[2],
            "Show date",
            self.show_date,
            focused == 2,
            controls::UiStyle { alpha: 1.0 },
        );

        false
    }

    fn handle_key(&mut self, key: nannou::prelude::Key, _ctrl: bool, _shift: bool) -> bool {
        // Control-mode keyboard UI:
        // - Up/Down moves focus
        // - Left/Right changes the focused value
        // - Enter/Space toggles/activates
        let mut focused = self.focused_control.lock().map(|v| *v).unwrap_or(0);

        match key {
            nannou::prelude::Key::Up => {
                focused = focused.saturating_sub(1);
            }
            nannou::prelude::Key::Down => {
                focused = (focused + 1).min(2);
            }
            nannou::prelude::Key::Left | nannou::prelude::Key::Right => match focused {
                0 => {
                    self.format = match self.format {
                        TimeFormat::TwentyFourHour => TimeFormat::TwelveHour,
                        TimeFormat::TwelveHour => TimeFormat::TwentyFourHour,
                    };
                }
                1 => self.show_seconds = !self.show_seconds,
                2 => self.show_date = !self.show_date,
                _ => {}
            },
            nannou::prelude::Key::Return | nannou::prelude::Key::Space => match focused {
                0 => {
                    self.format = match self.format {
                        TimeFormat::TwentyFourHour => TimeFormat::TwelveHour,
                        TimeFormat::TwelveHour => TimeFormat::TwentyFourHour,
                    };
                }
                1 => self.show_seconds = !self.show_seconds,
                2 => self.show_date = !self.show_date,
                _ => {}
            },
            _ => return false,
        }

        if let Ok(mut guard) = self.focused_control.lock() {
            *guard = focused;
        }
        true
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
            }
            "toggle_seconds" => {
                self.show_seconds = !self.show_seconds;
                true
            }
            _ => false,
        }
    }

    fn get_display_text(&self) -> Option<String> {
        Some(self.current_time.clone())
    }
}
