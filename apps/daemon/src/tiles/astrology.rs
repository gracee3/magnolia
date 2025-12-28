//! Astrology Tile - Sun and Moon positions with zodiac signs

use nannou::prelude::*;
use chrono::{Local, Timelike, Datelike};
use super::{TileRenderer, RenderContext};

pub struct AstroTile {
    sun_longitude: f64,
    moon_longitude: f64,
    sun_sign: String,
    moon_sign: String,
    last_update: std::time::Instant,
}

impl AstroTile {
    pub fn new() -> Self {
        let mut tile = Self {
            sun_longitude: 0.0,
            moon_longitude: 0.0,
            sun_sign: String::new(),
            moon_sign: String::new(),
            last_update: std::time::Instant::now(),
        };
        tile.calculate_positions();
        tile
    }
    
    fn calculate_positions(&mut self) {
        // Simplified astrology calculation (approximate)
        // For real calculations, use aphrodite crate
        let now = Local::now();
        let day_of_year = now.ordinal() as f64;
        let hour = now.hour() as f64 + now.minute() as f64 / 60.0;
        
        // Approximate sun position (moves ~1° per day)
        // Spring equinox (March 20) = 0° Aries
        // Offset: day 79 = 0° Aries
        self.sun_longitude = ((day_of_year - 79.0) * (360.0 / 365.25)) % 360.0;
        if self.sun_longitude < 0.0 { self.sun_longitude += 360.0; }
        
        // Approximate moon position (moves ~13° per day)
        // This is a rough approximation
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
    
    fn render(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.08, 0.9));
        
        let line_height = rect.h() / 4.0;
        let font_size = (line_height * 0.6).min(24.0) as u32;
        
        // Sun line
        let sun_text = format!("☉ Sun: {:.1}° {}", self.sun_longitude, self.sun_sign);
        draw.text(&sun_text)
            .xy(pt2(rect.x(), rect.y() + line_height * 0.5))
            .color(srgb(1.0, 0.8, 0.2))
            .font_size(font_size);
        
        // Moon line  
        let moon_text = format!("☽ Moon: {:.1}° {}", self.moon_longitude, self.moon_sign);
        draw.text(&moon_text)
            .xy(pt2(rect.x(), rect.y() - line_height * 0.5))
            .color(srgb(0.8, 0.8, 1.0))
            .font_size(font_size);
        
        // Label
        draw.text("ASTRO")
            .xy(pt2(rect.x(), rect.top() - 20.0))
            .color(srgba(0.5, 0.5, 0.5, 1.0))
            .font_size(12);
    }
    
    fn get_display_text(&self) -> Option<String> {
        Some(format!(
            "Sun: {:.1}° {} | Moon: {:.1}° {}",
            self.sun_longitude, self.sun_sign,
            self.moon_longitude, self.moon_sign
        ))
    }
}
