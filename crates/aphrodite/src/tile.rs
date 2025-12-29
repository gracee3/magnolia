//! Astrology Tile - Full astrological chart rendering
//!
//! Monitor mode: Shows current sun/moon positions
//! Control mode: Full astrological wheel chart with planets, houses, and signs

use nannou::prelude::*;
use nannou_egui::egui;
use chrono::Utc;
use talisman_core::{TileRenderer, RenderContext, BindableAction};
use std::collections::HashMap;

use crate::ephemeris::{SwissEphemerisAdapter, EphemerisSettings, GeoLocation, LayerPositions};
use crate::layout::{load_wheel_definition_from_json, WheelAssembler};
use crate::rendering::{ChartSpecGenerator, ChartSpec};
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
    
    fn build_spec(&self, w: f32, h: f32) -> Option<ChartSpec> {
        let positions = self.last_positions.as_ref()?;
        
        let wheel_def = match load_wheel_definition_from_json(DEFAULT_WHEEL_JSON) {
            Ok(def) => def,
            Err(_) => return None,
        };
        
        let mut positions_by_layer = HashMap::new();
        positions_by_layer.insert("now".to_string(), positions.clone());
        
        let empty_aspects: HashMap<String, crate::aspects::types::AspectSet> = HashMap::new();
        
        let wheel = WheelAssembler::build_wheel(
            &wheel_def.wheel,
            &positions_by_layer,
            &empty_aspects,
            Some(&self.eph_settings.include_objects),
        );
        
        let gen = ChartSpecGenerator::new();
        Some(gen.generate(&wheel, &empty_aspects, w, h))
    }
}

impl Default for AstroTile {
    fn default() -> Self {
        Self::new()
    }
}

// === Nannou Drawing Bridge ===

/// Convert aphrodite Color to nannou Srgba
fn to_srgba(c: PrimColor) -> Srgba {
    srgba(
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        c.a as f32 / 255.0,
    )
}

/// Map ChartSpec point to nannou Rect coordinates
/// ChartSpec uses top-left origin with y-down; nannou uses center origin with y-up
fn map_point(spec: &ChartSpec, rect: Rect, p: PrimPoint) -> Point2 {
    // Convert from spec's coordinate system (0,0 = top-left, y-down) to nannou (center-origin, y-up)
    let x_offset = p.x - spec.width / 2.0;
    let y_offset = (spec.height / 2.0) - p.y; // Flip y-axis
    pt2(rect.x() + x_offset, rect.y() + y_offset)
}

/// Generate polygon points for a ring segment (arc-based)
fn ring_segment_points(
    center: Point2,
    r_in: f32,
    r_out: f32,
    start_deg: f32,
    end_deg: f32,
    steps: usize,
) -> Vec<Point2> {
    let a0 = start_deg;
    let mut a1 = end_deg;
    if a1 < a0 {
        a1 += 360.0;
    }
    
    let steps = steps.max(6);
    let mut outer = Vec::with_capacity(steps + 1);
    let mut inner = Vec::with_capacity(steps + 1);
    
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let a_deg = a0 + (a1 - a0) * t;
        // ChartSpec uses 0° = top, clockwise; convert to math angle (0° = right, CCW)
        let a_rad = (90.0 - a_deg).to_radians();
        
        outer.push(pt2(center.x + r_out * a_rad.cos(), center.y + r_out * a_rad.sin()));
        inner.push(pt2(center.x + r_in * a_rad.cos(), center.y + r_in * a_rad.sin()));
    }
    
    // Close the polygon: outer arc -> inner arc (reversed)
    inner.reverse();
    outer.extend(inner);
    outer
}

/// Draw a ChartSpec to nannou Draw
fn draw_spec(draw: &Draw, rect: Rect, spec: &ChartSpec) {
    // Background
    draw.rect()
        .xy(rect.xy())
        .wh(rect.wh())
        .color(to_srgba(spec.background_color));
    
    for shape in &spec.shapes {
        match shape {
            Shape::Circle { center, radius, fill, stroke: _ } => {
                let c = map_point(spec, rect, *center);
                if let Some(f) = fill {
                    draw.ellipse()
                        .xy(c)
                        .radius(*radius)
                        .color(to_srgba(*f));
                }
            }
            Shape::Line { from, to, stroke } => {
                draw.line()
                    .points(map_point(spec, rect, *from), map_point(spec, rect, *to))
                    .weight(stroke.width)
                    .color(to_srgba(stroke.color));
            }
            Shape::Text { position, content, size, color, .. } => {
                draw.text(content)
                    .xy(map_point(spec, rect, *position))
                    .font_size((*size).max(8.0) as u32)
                    .color(to_srgba(*color));
            }
            Shape::SignSegment {
                center,
                start_angle,
                end_angle,
                radius_inner,
                radius_outer,
                fill,
                ..
            } => {
                let c = map_point(spec, rect, *center);
                let pts = ring_segment_points(c, *radius_inner, *radius_outer, *start_angle, *end_angle, 48);
                if !pts.is_empty() {
                    draw.polygon()
                        .points(pts)
                        .color(to_srgba(*fill));
                }
            }
            Shape::PlanetGlyph {
                center,
                planet_id,
                size,
                color,
                retrograde: _,
            } => {
                let c = map_point(spec, rect, *center);
                // Planet glyph mapping
                let glyph = match planet_id.as_str() {
                    "sun" => "☉",
                    "moon" => "☽",
                    "mercury" => "☿",
                    "venus" => "♀",
                    "mars" => "♂",
                    "jupiter" => "♃",
                    "saturn" => "♄",
                    "uranus" => "♅",
                    "neptune" => "♆",
                    "pluto" => "♇",
                    "asc" => "ASC",
                    "mc" => "MC",
                    "ic" => "IC",
                    "dc" => "DC",
                    _ => planet_id,
                };
                draw.text(glyph)
                    .xy(c)
                    .font_size((*size).max(10.0) as u32)
                    .color(to_srgba(*color));
            }
            _ => {
                // Other shapes (Arc, Path, HouseSegment, AspectLine) can be added later
            }
        }
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
    
    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        // Draw the full astrological chart in maximized mode
        if let Some(spec) = self.build_spec(rect.w(), rect.h()) {
            draw_spec(draw, rect, &spec);
        } else {
            // Fallback if chart generation fails
            draw.rect()
                .xy(rect.xy())
                .wh(rect.wh())
                .color(BLACK);
            draw.text("No ephemeris data available")
                .xy(rect.xy())
                .color(WHITE)
                .font_size(16);
        }
        
        // Optional egui overlay for settings
        let mut used_egui = false;
        if let Some(egui_ctx) = ctx.egui_ctx {
            used_egui = true;
            
            egui::Window::new("Astro Chart")
                .collapsible(false)
                .resizable(true)
                .default_size(egui::vec2(250.0, 200.0))
                .show(egui_ctx, |ui| {
                    ui.heading("Current Positions");
                    ui.separator();
                    ui.label(format!("☉ Sun: {:.2}° {}", self.sun_longitude, self.sun_sign));
                    ui.label(format!("☽ Moon: {:.2}° {}", self.moon_longitude, self.moon_sign));
                    ui.add_space(10.0);
                    ui.separator();
                    ui.label("Settings");
                    let mut show_deg = self.show_degrees;
                    let mut show_moon = self.show_moon;
                    ui.checkbox(&mut show_deg, "Show Degrees");
                    ui.checkbox(&mut show_moon, "Show Moon");
                    // Note: Changes would need to be applied via apply_settings in a real implementation
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
