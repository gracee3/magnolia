//! Astrology Tile - Full astrological chart rendering
//!
//! Monitor mode: Shows current sun/moon positions
//! Control mode: Full astrological wheel chart with planets, houses, and signs

use nannou::prelude::*;
// use nannou_egui::egui removed
use chrono::Utc;
use talisman_core::{TileRenderer, RenderContext, BindableAction};
use std::collections::HashMap;

use crate::ephemeris::{SwissEphemerisAdapter, EphemerisSettings, GeoLocation, LayerPositions};
// use crate::layout::{load_wheel_definition_from_json, WheelAssembler}; // Deprecated
// use crate::rendering::{ChartSpecGenerator, ChartSpec}; // Deprecated
use crate::chart::{RadixChart, TransitChart, ChartSettings, ChartData};
use crate::rendering::glyphs::Glyph;
use crate::rendering::primitives::{Shape, Color as PrimColor, Point as PrimPoint};

const DEFAULT_WHEEL_JSON: &str = r#"
{
  "name": "Talisman Wheel",
  "rings": [
    {
      "slug": "ring_signs",
      "type": "signs",
      "label": "Zodiac Signs",
      "orderIndex": 0,
      "radiusInner": 0.78,
      "radiusOuter": 0.98,
      "dataSource": { "kind": "static_zodiac" }
    },
    {
      "slug": "ring_houses",
      "type": "houses",
      "label": "Houses",
      "orderIndex": 1,
      "radiusInner": 0.70,
      "radiusOuter": 0.78,
      "dataSource": { "kind": "layer_houses", "layerId": "now" }
    },
    {
      "slug": "ring_planets",
      "type": "planets",
      "label": "Planets",
      "orderIndex": 2,
      "radiusInner": 0.52,
      "radiusOuter": 0.70,
      "dataSource": { "kind": "layer_planets", "layerId": "now" }
    }
  ]
}
"#;

pub struct AstroTile {
    // Ephemeris state
    adapter: Option<SwissEphemerisAdapter>,
    eph_settings: EphemerisSettings,
    location: Option<GeoLocation>,
    last_positions: Option<LayerPositions>,
    
    // Display state
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
        let adapter = SwissEphemerisAdapter::new(None).ok();
        
        let eph_settings = EphemerisSettings {
            zodiac_type: "tropical".to_string(),
            ayanamsa: None,
            house_system: "placidus".to_string(),
            include_objects: vec![
                "sun".to_string(),
                "moon".to_string(),
                "mercury".to_string(),
                "venus".to_string(),
                "mars".to_string(),
                "jupiter".to_string(),
                "saturn".to_string(),
                "uranus".to_string(),
                "neptune".to_string(),
                "pluto".to_string(),
            ],
        };
        
        let mut tile = Self {
            adapter,
            eph_settings,
            location: Some(GeoLocation { lat: 51.48, lon: 0.0 }), // Greenwich default
            last_positions: None,
            sun_longitude: 0.0,
            moon_longitude: 0.0,
            sun_sign: String::new(),
            moon_sign: String::new(),
            last_update: std::time::Instant::now(),
            show_degrees: true,
            show_moon: true,
        };
        
        tile.refresh_ephemeris();
        tile
    }
    
    fn refresh_ephemeris(&mut self) {
        let now = Utc::now();
        let Some(adapter) = self.adapter.as_mut() else { return; };
        
        match adapter.calc_positions(now, self.location.clone(), &self.eph_settings) {
            Ok(pos) => {
                self.sun_longitude = pos.planets.get("sun").map(|p| p.lon).unwrap_or(0.0);
                self.moon_longitude = pos.planets.get("moon").map(|p| p.lon).unwrap_or(0.0);
                self.sun_sign = Self::longitude_to_sign(self.sun_longitude);
                self.moon_sign = Self::longitude_to_sign(self.moon_longitude);
                self.last_positions = Some(pos);
            }
            Err(_) => {
                // Keep last known positions on error
            }
        }
    }
    
    fn longitude_to_sign(longitude: f64) -> String {
        let signs = [
            "Aries ♈", "Taurus ♉", "Gemini ♊", "Cancer ♋",
            "Leo ♌", "Virgo ♍", "Libra ♎", "Scorpio ♏",
            "Sagittarius ♐", "Capricorn ♑", "Aquarius ♒", "Pisces ♓"
        ];
        let normalized = if longitude < 0.0 { longitude + 360.0 } else { longitude };
        let index = ((normalized / 30.0).floor() as usize) % 12;
        signs[index].to_string()
    }
    
    // fn build_spec removed
}

impl Default for AstroTile {
    fn default() -> Self {
        Self::new()
    }
}

impl TileRenderer for AstroTile {
    fn id(&self) -> &str {
        "astro"
    }
    
    fn name(&self) -> &str {
        "Astrology"
    }
    
    fn update(&mut self) {
        // Update every 10 seconds
        if self.last_update.elapsed().as_secs() >= 10 {
            self.refresh_ephemeris();
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
    
    fn render_controls(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) -> bool {
        // Draw the full astrological chart in maximized mode
        let Some(positions) = &self.last_positions else {
            draw.rect().xy(rect.xy()).wh(rect.wh()).color(BLACK);
            draw.text("No data").xy(rect.xy()).color(WHITE);
            return false;
        };

        // Convert data
        let data: ChartData = positions.into();
        
        // Setup Chart
        // TODO: Load settings from tile configuration or user prefs
        let settings = ChartSettings::default();
        
        let min_dim = rect.w().min(rect.h());
        let radius = min_dim / 2.0 - settings.margin; // Margin
        
        // Create Radix Chart
        let radix = RadixChart::new(rect.x(), rect.y(), radius, &data, &settings);
        
        // Draw Layers
        radix.draw_bg(draw);
        radix.draw_universe(draw);
        radix.draw_cusps(draw);
        radix.draw_axis(draw);
        radix.draw_points(draw);
        
        // TODO: Draw transit if available
        // let transit = TransitChart::new(...);
        // transit.draw_points(draw);
        // transit.draw_cusps(draw);
        
        false
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
            }
            "toggle_moon" => {
                self.show_moon = !self.show_moon;
                true
            }
            "refresh" => {
                self.refresh_ephemeris();
                self.last_update = std::time::Instant::now();
                true
            }
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
