//! Astrology Tile - Sun and Moon positions with zodiac signs
//!
//! Monitor mode: Shows current sun/moon positions
//! Control mode: Settings for which planets to display

use nannou::prelude::*;
use nannou_egui::egui;
use chrono::{Local, Timelike, Datelike};
use talisman_core::{TileRenderer, RenderContext, BindableAction};

pub struct AstroTile {
    sun_longitude: f64,
    moon_longitude: f64,
    sun_sign: String,
    moon_sign: String,
    last_update: std::time::Instant,
    show_degrees: bool,
    show_moon: bool,
}

impl AstroTile {
    pub fn new() -> Self {
        let mut tile = Self {
            sun_longitude: 0.0,
            moon_longitude: 0.0,
            sun_sign: String::new(),
            moon_sign: String::new(),
            last_update: std::time::Instant::now(),
            show_degrees: true,
            show_moon: true,
        };
        tile.calculate_positions();
        tile
    }
    
    fn calculate_positions(&mut self) {
        // Simplified astrology calculation (approximate)
        // For real calculations, use the aphrodite ephemeris module
        let now = Local::now();
        let day_of_year = now.ordinal() as f64;
        let hour = now.hour() as f64 + now.minute() as f64 / 60.0;
        
        // Approximate sun position (moves ~1° per day)
        // Spring equinox (March 20) = 0° Aries
        // Offset: day 79 = 0° Aries
        self.sun_longitude = ((day_of_year - 79.0) * (360.0 / 365.25)) % 360.0;
        if self.sun_longitude < 0.0 { self.sun_longitude += 360.0; }
        
        // Approximate moon position (moves ~13° per day)
        let days_since_new_moon = (day_of_year + hour / 24.0) % 29.53;
        self.moon_longitude = (days_since_new_moon * (360.0 / 29.53) + self.sun_longitude) % 360.0;
        
        self.sun_sign = Self::longitude_to_sign(self.sun_longitude);
        self.moon_sign = Self::longitude_to_sign(self.moon_longitude);
    }
    
    fn longitude_to_sign(longitude: f64) -> String {
        let signs = [
            "Aries ♈", "Taurus ♉", "Gemini ♊", "Cancer ♋",
            "Leo ♌", "Virgo ♍", "Libra ♎", "Scorpio ♏",
            "Sagittarius ♐", "Capricorn ♑", "Aquarius ♒", "Pisces ♓"
        ];
        let index = ((longitude / 30.0) as usize) % 12;
        signs[index].to_string()
    }
}

impl Default for AstroTile {
    fn default() -> Self {
        Self::new()
    }
}

impl TileRenderer for AstroTile {
    fn id(&self) -> &str { "astro" }
    
    fn name(&self) -> &str { "Astrology" }
    
    fn update(&mut self) {
        // Update every 60 seconds
        if self.last_update.elapsed().as_secs() >= 60 {
            self.calculate_positions();
            self.last_update = std::time::Instant::now();
        }
    }
    
    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.08, 0.9));
        
        let line_height = rect.h() / 4.0;
        let font_size = (line_height * 0.6).min(24.0) as u32;
        
        // Sun line
        let sun_text = if self.show_degrees {
            format!("☉ Sun: {:.1}° {}", self.sun_longitude, self.sun_sign)
        } else {
            format!("☉ Sun in {}", self.sun_sign)
        };
        draw.text(&sun_text)
            .xy(pt2(rect.x(), rect.y() + line_height * 0.5))
            .color(srgb(1.0, 0.8, 0.2))
            .font_size(font_size);
        
        // Moon line
        if self.show_moon {
            let moon_text = if self.show_degrees {
                format!("☽ Moon: {:.1}° {}", self.moon_longitude, self.moon_sign)
            } else {
                format!("☽ Moon in {}", self.moon_sign)
            };
            draw.text(&moon_text)
                .xy(pt2(rect.x(), rect.y() - line_height * 0.5))
                .color(srgb(0.8, 0.8, 1.0))
                .font_size(font_size);
        }
        
        // Label
        draw.text("ASTRO")
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
        draw.text("ASTROLOGY SETTINGS")
            .xy(pt2(rect.x(), rect.top() - 30.0))
            .color(CYAN)
            .font_size(18);
        
        // Large display preview
        let preview_rect = Rect::from_x_y_w_h(
            rect.x(),
            rect.y() + 80.0,
            rect.w() * 0.8,
            rect.h() * 0.4,
        );
        self.render_monitor(draw, preview_rect, ctx);
        
        // Egui controls
        let mut used_egui = false;
        if let Some(egui_ctx) = ctx.egui_ctx {
            used_egui = true;
            
            let panel_x = rect.left() + 60.0 + (rect.w() / 2.0);
            let panel_y = rect.y() + 40.0 + (rect.h() / 2.0);
            
            egui::Area::new(egui::Id::new("astro_controls"))
                .fixed_pos(egui::pos2(panel_x, panel_y))
                .show(egui_ctx, |ui| {
                    ui.set_max_width(250.0);
                    
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgba_unmultiplied(10, 10, 15, 240))
                        .inner_margin(egui::Margin::same(15.0))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 100, 100)))
                        .show(ui, |ui| {
                            ui.heading(egui::RichText::new("Display Options").color(egui::Color32::from_rgb(0, 255, 255)));
                            ui.add_space(10.0);
                            
                            let mut show_deg = self.show_degrees;
                            ui.checkbox(&mut show_deg, "Show Degrees");
                            
                            let mut show_m = self.show_moon;
                            ui.checkbox(&mut show_m, "Show Moon");
                            
                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(5.0);
                            
                            ui.label(egui::RichText::new("Current Positions").color(egui::Color32::GRAY).small());
                            ui.label(format!("☉ {:.2}°", self.sun_longitude));
                            ui.label(format!("☽ {:.2}°", self.moon_longitude));
                        });
                });
        }
        
        used_egui
    }
    
    fn settings_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "show_degrees": {
                    "type": "boolean",
                    "default": true
                },
                "show_moon": {
                    "type": "boolean",
                    "default": true
                }
            }
        }))
    }
    
    fn apply_settings(&mut self, settings: &serde_json::Value) {
        if let Some(d) = settings.get("show_degrees").and_then(|v| v.as_bool()) {
            self.show_degrees = d;
        }
        if let Some(m) = settings.get("show_moon").and_then(|v| v.as_bool()) {
            self.show_moon = m;
        }
    }
    
    fn get_settings(&self) -> serde_json::Value {
        serde_json::json!({
            "show_degrees": self.show_degrees,
            "show_moon": self.show_moon
        })
    }
    
    fn bindable_actions(&self) -> Vec<BindableAction> {
        vec![
            BindableAction::new("toggle_degrees", "Toggle Degrees", true),
            BindableAction::new("toggle_moon", "Toggle Moon", true),
            BindableAction::new("refresh", "Refresh Positions", false),
        ]
    }
    
    fn execute_action(&mut self, action: &str) -> bool {
        match action {
            "toggle_degrees" => {
                self.show_degrees = !self.show_degrees;
                true
            },
            "toggle_moon" => {
                self.show_moon = !self.show_moon;
                true
            },
            "refresh" => {
                self.calculate_positions();
                self.last_update = std::time::Instant::now();
                true
            },
            _ => false,
        }
    }
    
    fn get_display_text(&self) -> Option<String> {
        Some(format!(
            "Sun: {:.1}° {} | Moon: {:.1}° {}",
            self.sun_longitude, self.sun_sign,
            self.moon_longitude, self.moon_sign
        ))
    }
}
